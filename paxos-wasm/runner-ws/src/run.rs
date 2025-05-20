use std::{error::Error, sync::Arc, thread};
use tokio::runtime::Runtime;
use tracing::info;
use wasmtime::Engine;

use crate::{config::Config, host_logger, paxos_wasm::PaxosWasmtime};

pub async fn run_standalone(
    node_id: u64,
    config: String,
    engine: &Engine,
) -> Result<(), Box<dyn Error>> {
    let cfg = Config::load(config, node_id);
    info!(
        "Node {} @{} role={:?} is_leader={}",
        cfg.node.node_id, cfg.node.address, cfg.node.role, cfg.is_leader,
    );

    host_logger::init_tracing_with(cfg.log_level);

    let paxos = PaxosWasmtime::new(
        engine,
        cfg.node.clone(),
        cfg.remote_nodes.clone(),
        cfg.is_leader,
        cfg.run_config.clone(),
        cfg.log_level,
    )
    .await?;

    let mut store = paxos.store.lock().await;
    paxos
        .resource()
        .call_run(&mut *store, paxos.resource_handle.clone())
        .await?;

    Ok(())
}

pub async fn run_same_runtime(
    base_node_id: u64,
    config: String,
    engine: &Engine,
) -> Result<(), Box<dyn Error>> {
    let mut tasks = vec![];

    for offset in 0..3 {
        let node_id = base_node_id + offset;
        let engine = engine.clone(); // Clone the Arc for thread safety
        let config = config.clone(); // Clone the config string

        // Spawn a new Tokio task to spawn a thread for each node
        let task = tokio::spawn(async move {
            let cfg = Config::load(config, node_id);
            info!(
                "Node {} @{} role={:?} is_leader={}",
                cfg.node.node_id, cfg.node.address, cfg.node.role, cfg.is_leader,
            );

            host_logger::init_tracing_with(cfg.log_level);

            let paxos_wasmtime = Arc::new(
                PaxosWasmtime::new(
                    &engine,
                    cfg.node.clone(),
                    cfg.remote_nodes.clone(),
                    cfg.is_leader,
                    cfg.run_config.clone(),
                    cfg.log_level,
                )
                .await
                .expect("Failed to initialize PaxosWasmtime"),
            );

            // Offload the async code to a separate OS thread using a Tokio runtime in that thread
            let paxos_wasmtime_clone = paxos_wasmtime.clone();
            thread::spawn(move || {
                // Create a new Tokio runtime in the OS thread
                let rt = Runtime::new().expect("Failed to create Tokio runtime");

                // Run the async `call_run` within the Tokio runtime in the OS thread
                rt.block_on(async {
                    let mut guard = paxos_wasmtime_clone.store.lock().await;
                    let store_ctx = &mut *guard;

                    let resource = paxos_wasmtime_clone.resource();

                    // Directly call the infinite `call_run` method without an additional loop
                    resource
                        .call_run(store_ctx, paxos_wasmtime_clone.resource_handle.clone())
                        .await
                        .expect("Failed to call run");
                });
            });
        });

        tasks.push(task);
    }

    // Wait for all tasks to be spawned (but they will run indefinitely)
    futures::future::join_all(tasks).await;

    // Wait for a SIGINT (Ctrl+C) to gracefully shutdown the program
    tokio::signal::ctrl_c().await?; // Wait for SIGINT (Ctrl+C)
    Ok(())
}
