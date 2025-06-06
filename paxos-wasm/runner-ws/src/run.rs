use crate::Args;
use crate::config::Config;
use crate::host_logger;
use crate::paxos_wasm::PaxosWasmtime;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::{sync::mpsc, time::sleep};
use tracing::{error, warn};
use wasmtime::Engine;

use once_cell::sync::Lazy;

static START_TIME: Lazy<Mutex<Instant>> = Lazy::new(|| Mutex::new(Instant::now()));

const MAX_RETRIES: i32 = 5;

// Total allowed downtime before forcing restart: (failure_check_interval × 2) − 1
const TOTAL_TIMEOUT_INTERVALS: u64 = 9; // Could be higher on real servers due to possible overlapping client-servers
const GRACE_INTERVALS: u64 = 2;
const SHUTDOWN_TIMEOUT_INTERVALS: u64 = TOTAL_TIMEOUT_INTERVALS - GRACE_INTERVALS;

fn reset_start_time() {
    *START_TIME.lock().unwrap() = Instant::now();
}

fn elapsed_since_start() {
    warn!(
        "[Host] Time from detecting error to calling run on new component: {:?}",
        START_TIME.lock().unwrap().elapsed()
    );
}

pub async fn run_cluster(args: &Args, engine: &Engine) -> anyhow::Result<()> {
    let cfg = Config::load(&args.config, args.cluster_id)?;
    host_logger::init_tracing_with(cfg.log_level);

    error!(
        "Cluster {}: {} nodes, leader={}",
        args.cluster_id,
        cfg.cluster_nodes.len(),
        cfg.leader_id,
    );

    for node in &cfg.cluster_nodes {
        let role = if node.node_id == cfg.leader_id {
            "Leader"
        } else {
            "Node"
        };
        error!(
            "  • Node {} @{} ({:?} {})",
            node.node_id, node.address, node.role, role,
        );
    }

    let mut tasks = Vec::new();
    let run_config_template = cfg.run_config.clone();
    let wasm_path = args.wasm.clone();
    let logs_dir_base = args.logs_dir.as_ref().map(PathBuf::from);

    for node in cfg.cluster_nodes {
        let engine = engine.clone();
        let is_leader = node.node_id == cfg.leader_id;
        let mut run_config = run_config_template.clone();
        let log_level = cfg.log_level;
        let node_wasm_path = wasm_path.clone();
        let node_logs_dir = logs_dir_base.clone();

        let other_active = cfg
            .active_nodes
            .iter()
            .filter(|n2| n2.node_id != node.node_id)
            .cloned()
            .collect::<Vec<_>>();

        tasks.push(tokio::spawn(async move {
            // Builder task: keep one Paxos instance ready.
            let (tx, mut rx) = mpsc::channel::<PaxosWasmtime>(1);
            let builder_node = node.clone();
            let builder_engine = engine.clone();
            let builder_wasm = node_wasm_path.clone();
            let builder_logs = node_logs_dir.clone();
            let builder_level = log_level;

            tokio::spawn(async move {
                let mut retries = 0;
                loop {
                    if tx.capacity() > 0 {
                        match PaxosWasmtime::new(
                            &builder_engine,
                            &builder_node,
                            builder_level,
                            builder_wasm.clone(),
                            builder_logs.as_deref(),
                        )
                        .await
                        {
                            Ok(paxos) => {
                                if tx.send(paxos).await.is_ok() {
                                    warn!(
                                        "[Host] Node {}: Prebuilt new Paxos instance",
                                        builder_node.node_id
                                    );
                                }
                                retries = 0;
                            }
                            Err(e) => {
                                error!(
                                    "[Host] Node {}: Error building Paxos: {:?}",
                                    builder_node.node_id, e
                                );
                                retries += 1;
                                if retries >= MAX_RETRIES {
                                    error!(
                                        "[Host] Node {}: Reached max builder retries ({}). Exiting builder task.",
                                        builder_node.node_id, MAX_RETRIES
                                    );
                                    break;
                                }
                            }
                        }
                    }
                    sleep(Duration::from_secs(1)).await;
                }
            });

            // Main loop: run instances, monitor, restart on failure
            let mut main_retries = 0;
            loop {
                let mut paxos = match rx.recv().await {
                    Some(p) => {
                        warn!("[Host] Node {}: Got prebuilt Paxos instance.", node.node_id);
                        p
                    }
                    None => {
                        error!(
                            "[Host] Node {}: Builder channel closed. Exiting loop.",
                            node.node_id
                        );
                        break;
                    }
                };

                let control = paxos.control_handle.clone();
                let heartbeat_ms = run_config.heartbeat_interval_ms;
                let shutdown_threshold = heartbeat_ms.saturating_mul(SHUTDOWN_TIMEOUT_INTERVALS);
                let node_id = node.node_id;

                // Monitor: wait for first ping, then check timeout every heartbeat interval.
                let mut monitor_handle = {
                    let control = control.clone();
                    tokio::spawn(async move {
                        while control.last_ping_time().await.is_none() {
                            sleep(Duration::from_millis(heartbeat_ms)).await;
                        }
                        loop {
                            sleep(Duration::from_millis(heartbeat_ms)).await;
                            if let Some(ts) = control.last_ping_time().await {
                                let elapsed = Instant::now().duration_since(ts);
                                if elapsed.as_millis() as u64 > shutdown_threshold {
                                    warn!(
                                        "[Host Monitor] Node {} hasn’t pinged in {:?}, requesting shutdown.",
                                        node_id, elapsed
                                    );
                                    control.request_shutdown().await;
                                    break;
                                }
                            }
                        }
                    })
                };

                // Runner: actually run the Paxos instance.
                let mut run_handle = {
                    let node = node.clone();
                    let all_nodes = other_active.clone();
                    let run_config = run_config.clone();
                    tokio::spawn(async move {
                        warn!("[Host] Node {}: Starting Paxos instance.", node.node_id);
                        elapsed_since_start();
                        paxos.run(node, all_nodes, is_leader, run_config).await
                    })
                };

                // Wait for run() to finish or monitor to request shutdown
                let shutdown_requested = {
                    tokio::select! {
                        run_res = &mut run_handle => {
                            // Runner finished first
                            if !monitor_handle.is_finished() {
                                monitor_handle.abort();
                            }
                            match run_res {
                                Ok(Ok(())) => {
                                    warn!(
                                        "[Host] Node {}: Paxos exited cleanly. Restarting loop.",
                                        node.node_id
                                    );
                                    continue; // reload a fresh instance without incrementing retries
                                }
                                Ok(Err(e)) => {
                                    reset_start_time();
                                    warn!(
                                        "[Host] Node {}: Paxos error: {:?}. Retrying.",
                                        node.node_id, e
                                    );
                                    true
                                }
                                Err(join_err) => {
                                    reset_start_time();
                                    warn!(
                                        "[Host] Node {}: Paxos panicked: {:?}. Retrying.",
                                        node.node_id, join_err
                                    );
                                    true
                                }
                            }
                        }
                        _ = &mut monitor_handle => {
                            // Monitor fired first
                            true
                        }
                    }
                };

                if shutdown_requested {
                    // Grace period before forced abort
                    let grace_duration = Duration::from_millis(heartbeat_ms.saturating_mul(GRACE_INTERVALS));
                    warn!(
                        "[Host] Node {}: Shutdown requested; waiting {:?} before force‐abort.",
                        node.node_id, grace_duration
                    );
                    sleep(grace_duration).await;

                    if !run_handle.is_finished() {
                        warn!(
                            "[Host] Node {}: Grace period elapsed; forcing runner abort.",
                            node.node_id
                        );
                        run_handle.abort();
                    }

                    // Ensure monitor is stopped
                    if !monitor_handle.is_finished() {
                        monitor_handle.abort();
                    }

                    reset_start_time();
                    warn!(
                        "[Host] Node {}: Runner terminated. Retrying with new instance.",
                        node.node_id
                    );
                }

                main_retries += 1;
                if main_retries >= MAX_RETRIES {
                    error!(
                        "[Host] Node {}: Reached max main loop retries ({}). Giving up.",
                        node.node_id, MAX_RETRIES
                    );
                    loop {
                        // Just sleep forever instead of quitting, so we can continue to see terminal output
                        sleep(Duration::from_secs(60)).await;
                    }
                }
                if let Some(entry) = run_config.crashes.iter_mut().find(|c| c.get(0) == Some(&node.node_id)) {
                    if entry.len() > 1 {
                        entry.remove(1);
                        warn!(
                            "[Host] Node {}: Removed used crash slot after restart, remaining = {:?}",
                            node.node_id, entry
                        );
                    }
                }

                sleep(Duration::from_millis(100)).await;
            }
        }));
    }

    // Wait for all node tasks (they run indefinitely or exit on success)
    futures::future::join_all(tasks).await;

    Ok(())
}
