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

#[derive(Parser, Clone)]
struct Args {
    /// which cluster to run
    #[clap(long)]
    cluster_id: u64,

    /// path to config file
    #[clap(long, default_value = "config.yaml")]
    config: String,

    /// path to wasm file
    #[clap(
        long,
        default_value = "./target/wasm32-wasip2/release/final_composed_runner.wasm"
    )]
    wasm: String,

    /// Optional path to logs directory (overrides workspace default)
    #[clap(long)]
    logs_dir: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Configure Wasmtime for async host→guest calls
    let mut cfg = wasmtime::Config::default();
    cfg.async_support(true);
    let engine = Engine::new(&cfg)?;

    // single entrypoint which spins up all nodes in this cluster
    run::run_cluster(&args, &engine).await
}
