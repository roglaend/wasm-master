mod bindings;
mod config;
mod host_logger;
mod paxos_wasm;
mod run;

use clap::Parser;
use config::Config;
use paxos_wasm::PaxosWasmtime;
use run::{run_same_runtime, run_standalone};
use tracing::info;
use wasmtime::Engine;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    node_id: u64,

    #[clap(long, default_value = "config.yaml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    host_logger::init_tracing();
    let args = Args::parse();

    let mut wasm_config = wasmtime::Config::default();
    wasm_config.async_support(true);
    let engine = Engine::new(&wasm_config)?;

    // run_standalone(args.node_id, args.config, &engine).await?;

    // or

    run_same_runtime(args.node_id, args.config, &engine).await?;

    Ok(())
}
