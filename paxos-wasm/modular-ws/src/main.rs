mod bindings;
mod host_logger;
mod paxos_wasm;
mod standalone_config;

use std::sync::Arc;

use bindings::paxos::default::paxos_types::RunConfig;
use clap::Parser;
use paxos_wasm::PaxosWasmtime;
use standalone_config::Config;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Parser)]
struct Args {
    #[clap(long)]
    node_id: u64,
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = fmt().with_env_filter(filter).finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global tracing subscriber");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let args = Args::parse();

    let config = Config::new(args.node_id);
    let is_leader = config.node.node_id == config.leader_id;
    info!("Node: {:?}. Is leader: {}.", config.node, is_leader);

    let run_config = RunConfig {
        is_event_driven: config.is_event_driven,
        acceptors_send_learns: false, // TODO: Hardcoded
        prepare_timeout: 1000,        // TODO: Hardcoded to 1 sec for now.
        demo_client: true,
    };

    // Create the Arc<PaxosWasmtime>, making a thread safe reference to the underlying PaxosWasmtime instance
    let paxos_wasmtime = Arc::new(
        PaxosWasmtime::new(
            config.node.clone(),
            config.remote_nodes.clone(),
            is_leader,
            run_config,
        )
        .await?,
    );

    let mut guard = paxos_wasmtime.store.lock().await;
    let store_ctx = &mut *guard;

    let resource = paxos_wasmtime.resource();

    resource
        .call_run(store_ctx, paxos_wasmtime.resource_handle.clone())
        .await?;

    Ok(())
}
