mod config;
mod grpc_service;
mod host_logger;
mod host_messenger;
mod paxos_bindings;
mod paxos_wasm;
mod tests;
mod translation_layer;

use clap::Parser;
use config::Config;
use grpc_service::PaxosService;
use paxos_wasm::PaxosWasmtime;
use proto::paxos_proto;
use std::sync::{Arc, atomic::AtomicU32};
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
    let is_leader = config.node.node_id == config.leader_id;
    info!("Node: {:?}. Is leader: {}.", config.node, is_leader);

    let paxos_wasmtime =
        Arc::new(PaxosWasmtime::new(config.node, config.remote_nodes, is_leader).await?);
    let paxos_service = PaxosService {
        client_seq: Arc::new(AtomicU32::new(0)),
        paxos_wasmtime,
    };

    let addr = config.bind_addr.parse()?;
    info!("gRPC server listening on {}", addr);

    Server::builder()
        .add_service(paxos_proto::paxos_server::PaxosServer::new(paxos_service))
        .serve(addr)
        .await?;

    Ok(())
}
