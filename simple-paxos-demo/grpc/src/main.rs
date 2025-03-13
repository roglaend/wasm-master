mod config;
mod grpc_service;
mod network;
mod paxos_wasm;
mod shared;

use clap::{Parser, arg};
use config::Config;
use grpc_service::PaxosService;
use log::info;
use paxos_wasm::PaxosWasmtime;
use proto::paxos_proto;
use std::sync::Arc;
use tonic::transport::Server;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    node_id: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let args = Args::parse();

    let config = Config::new(args.node_id);
    let is_leader = config.node_id == config.leader_id;
    println!("Node id: {}. Is leader: {}.", config.node_id, is_leader);

    // Initialize WASM Paxos coordinator.
    let paxos_wasmtime = Arc::new(PaxosWasmtime::new(config.node_id, config.remote_nodes).await?);

    // Initialize gRPC service.
    let paxos_service = PaxosService { paxos_wasmtime };

    // Parse the bind address from the configuration.
    let addr = config.bind_addr.parse()?;

    info!("gRPC server listening on {}", addr);
    Server::builder()
        .add_service(paxos_proto::paxos_server::PaxosServer::new(paxos_service))
        .serve(addr)
        .await?;

    Ok(())
}
