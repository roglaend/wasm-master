use crate::config::Config;
use crate::host_logger;
use crate::paxos_wasm::PaxosWasmtime;
use futures::future::join_all;
use tokio::signal;
use tracing::error;
use wasmtime::Engine;

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
        error!("  • Node {} @{} ({})", n.node_id, n.address, role,);
    }

    // spawn one async task per node in the cluster
    let mut tasks = Vec::new();
    for node in cfg.cluster_nodes.iter().cloned() {
        let engine = engine.clone();
        let all_nodes = cfg.all_nodes.clone();
        let is_leader = node.node_id == cfg.leader_id;
        let run_config = cfg.run_config.clone();
        let log_level = cfg.log_level;

        tasks.push(tokio::spawn(async move {
            let paxos = PaxosWasmtime::new(
                &engine,
                node.clone(),
                all_nodes,
                is_leader,
                run_config.clone(),
                log_level,
            )
            .await
            .expect("failed to instantiate Wasm");

            // this future never returns until Stop, it drives the Paxos loop
            let mut store = paxos.store.lock().await;
            paxos
                .resource()
                .call_run(&mut *store, paxos.resource_handle.clone())
                .await
                .expect("Wasm run()");
        }));
    }

    // wait for all the Wasm‐hosts to start, runs forever
    join_all(tasks).await;

    // and also listen for Ctrl-C so we can shut down gracefully
    signal::ctrl_c().await?;

    Ok(())
}
