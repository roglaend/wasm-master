mod bindings;
mod config;
mod host_logger;
mod paxos_wasm;
mod run;

pub mod host_network_client;
pub mod host_network_server;
pub mod host_serializer;
pub mod host_storage;

use clap::Parser;
use wasmtime::Engine;

#[derive(Parser)]
struct Args {
    /// which cluster to run
    #[clap(long)]
    cluster_id: u64,

    /// path to config file
    #[clap(long, default_value = "config.yaml")]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Configure Wasmtime for async host→guest calls
    let mut cfg = wasmtime::Config::default();
    cfg.async_support(true);
    let engine = Engine::new(&cfg)?;

    // single entrypoint which spins up all nodes in this cluster
    run::run_cluster(args.cluster_id, &args.config, &engine).await
}
