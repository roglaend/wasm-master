use crate::config::Config;
use crate::host_logger;
use crate::paxos_wasm::PaxosWasmtime;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;
use tokio::{sync::mpsc, time::sleep};
use tracing::{error, warn};
use wasmtime::Engine;

use once_cell::sync::Lazy;

static START_TIME: Lazy<Mutex<Instant>> = Lazy::new(|| Mutex::new(Instant::now()));

fn reset_start_time() {
    let mut time = START_TIME.lock().unwrap();
    *time = Instant::now();
    // println!("Start time reset.");
}

fn elapsed_since_start() {
    let time = START_TIME.lock().unwrap();
    warn!(
        "[Host] Time from detecting error to calling run on new component: {:?}",
        time.elapsed()
    );
}

pub async fn run_cluster(
    cluster_id: u64,
    config_path: &str,
    engine: &Engine,
) -> anyhow::Result<()> {
    let cfg = Config::load(config_path, cluster_id)?;
    host_logger::init_tracing_with(cfg.log_level);

    error!(
        "Cluster {}: {} nodes, leader={}",
        cluster_id,
        cfg.cluster_nodes.len(),
        cfg.leader_id,
    );

    for n in &cfg.cluster_nodes {
        let role = if n.node_id == cfg.leader_id {
            "Leader"
        } else {
            "Node"
        };
        error!(
            "  • Node {} @{} ({:?} {})",
            n.node_id, n.address, n.role, role,
        );
    }

    let mut tasks = Vec::new();

    for node in cfg.cluster_nodes {
        let engine = engine.clone();
        let is_leader = node.node_id == cfg.leader_id;
        let run_config = cfg.run_config.clone();
        let log_level = cfg.log_level;

        let other_active_nodes: Vec<_> = cfg
            .active_nodes
            .clone()
            .into_iter()
            .filter(|n| n.node_id != node.node_id)
            .collect();

        tasks.push(tokio::spawn(async move {
            // Channel to transfer prebuilt Paxos instances
            let (tx, mut rx) = mpsc::channel::<PaxosWasmtime>(1);

            // Builder task
            let builder_tx = tx.clone();
            let builder_node = node.clone();
            let builder_engine = engine.clone();
            tokio::spawn(async move {
                loop {
                    if builder_tx.capacity() > 0 {
                        match PaxosWasmtime::new(&builder_engine, builder_node.clone(), log_level)
                            .await
                        {
                            Ok(paxos) => {
                                if builder_tx.send(paxos).await.is_ok() {
                                    warn!(
                                        "[Host] Node {}: Prebuilt new Paxos instance",
                                        builder_node.node_id
                                    );
                                }
                            }
                            Err(e) => {
                                error!(
                                    "[Host] Node {}: Error building Paxos: {:?}",
                                    builder_node.node_id, e
                                );
                            }
                        }
                    }
                    sleep(Duration::from_secs(1)).await;
                }
            });

            // Main loop — consume prebuilt instances, run them, retry on failure
            loop {
                let mut paxos = match rx.recv().await {
                    Some(p) => {
                        warn!("[Host] Node {}: Got prebuilt paxos instance.", node.node_id);
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

                let run_result = tokio::spawn({
                    let node = node.clone();
                    let all_nodes = other_active_nodes.clone();
                    let run_config = run_config.clone();
                    warn!("[Host] Node {}: Starting paxos instance.", node.node_id);
                    elapsed_since_start();
                    async move { paxos.run(node, all_nodes, is_leader, run_config).await }
                })
                .await;

                match run_result {
                    Ok(Ok(_)) => {
                        warn!(
                            "[Host] Node {}: Paxos exited cleanly. Exiting node loop.",
                            node.node_id
                        );
                        // TODO: We are here if the Paxos instance exited cleanly, which it will if we are
                        // hot-reloading. Should not break, but rather start the updated instance in next loop tick.
                        break;
                    }
                    Ok(Err(e)) => {
                        reset_start_time();
                        warn!(
                            "[Host] Node {}: Paxos error: {:?}. Retrying with new instance.",
                            node.node_id, e
                        );
                    }
                    Err(e) => {
                        reset_start_time();
                        warn!(
                            "[Host] Node {}: Paxos panicked: {:?}. Retrying with new instance.",
                            node.node_id, e
                        );
                    }
                }

                // sleep(Duration::from_millis(100)).await;
            }
        }));
    }

    // Wait for all node tasks (they run indefinitely or exit on success)
    futures::future::join_all(tasks).await;

    Ok(())
}
