mod config;
mod grpc_service;
mod host_logger;
mod host_messenger;
mod paxos_bindings;
mod paxos_wasm;
mod translation_layer;
mod tests;

use clap::Parser;
use config::Config;
use grpc_service::PaxosService;
use paxos_wasm::PaxosWasmtime;
use proto::paxos_proto;
use std::sync::Arc;
use tonic::transport::Server;

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
    let is_leader = config.node_id == config.leader_id;
    info!("Node id: {}. Is leader: {}.", config.node_id, is_leader);

    let paxos_wasmtime = Arc::new(PaxosWasmtime::new(config.node_id, config.remote_nodes).await?);
    let paxos_service = PaxosService { paxos_wasmtime };

    let addr = config.bind_addr.parse()?;
    info!("gRPC server listening on {}", addr);

    Server::builder()
        .add_service(paxos_proto::paxos_server::PaxosServer::new(paxos_service))
        .serve(addr)
        .await?;

    Ok(())
}
