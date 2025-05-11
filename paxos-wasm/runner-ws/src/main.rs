mod bindings;
mod config;
mod host_logger;
mod paxos_wasm;

use clap::Parser;
use config::Config;
use paxos_wasm::PaxosWasmtime;
use tracing::info;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    node_id: u64,

    #[clap(long, default_value = "config.yaml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let cfg = Config::load(&args.config, args.node_id);
    info!(
        "Node {} @{} role={:?} is_leader={}",
        cfg.node.node_id, cfg.node.address, cfg.node.role, cfg.is_leader,
    );

    host_logger::init_tracing_with(cfg.log_level);

    let paxos = PaxosWasmtime::new(
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
