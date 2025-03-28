mod config;
mod grpc_service;
mod host_logger;
mod host_messenger;
mod failure_service;
mod paxos_bindings;
mod paxos_wasm;
mod translation_layer;
mod tests;

use clap::Parser;
use config::Config;
use failure_service::FailureService;
use grpc_service::PaxosService;
use paxos_wasm::PaxosWasmtime;
use proto::paxos_proto;
use std::{sync::Arc, time::Duration};
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

    let endpoints_addr: Vec<String> = config
        .remote_nodes
        .iter()
        .map(|node| node.address.clone())
        .collect();

    let is_leader = config.node_id == config.leader_id;
    info!("Node id: {}. Is leader: {}.", config.node_id, is_leader);

    // Create the Arc<PaxosWasmtime>
    let paxos_wasmtime = Arc::new(PaxosWasmtime::new(config.node_id, config.remote_nodes).await?);

    // Create the PaxosService for gRPC
    let paxos_service = PaxosService {
        paxos_wasmtime: paxos_wasmtime.clone(),
    };

    // Wrap the FailureService in an Arc as well
    let failure_service = Arc::new(FailureService {
        paxos_wasmtime: paxos_wasmtime.clone(),
    });

    
    failure_service.start_heartbeat_sender(config.node_id, endpoints_addr.clone(), Duration::from_millis(1000)); // Send hearbeats every second
    
    failure_service.start_failure_service(Duration::from_secs(5)); // Check for failures every 5 seconds - Adjust as needed

    // Now run the gRPC server in the foreground
    let addr = config.bind_addr.parse()?;
    info!("gRPC server listening on {}", addr);

    Server::builder()
        .add_service(paxos_proto::paxos_server::PaxosServer::new(paxos_service))
        .serve(addr)
        .await?;

    Ok(())
}