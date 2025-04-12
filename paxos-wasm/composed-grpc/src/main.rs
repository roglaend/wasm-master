mod config;
mod coordinator_bindings;
mod failure_service;
mod grpc_service;
mod host_logger;
mod host_messenger;
mod paxos_wasm;
mod run_paxos_service;
mod traits;
mod translation_layer;

#[macro_use]
mod macro_translation;

use clap::Parser;
use config::Config;
use coordinator_bindings::paxos::default::paxos_types::RunConfig;
use failure_service::FailureService;
use grpc_service::PaxosService;
use paxos_wasm::PaxosWasmtime;
use proto::paxos_proto;
use run_paxos_service::RunPaxosService;
use std::sync::{Arc, atomic::AtomicU32};
use std::time::Duration;
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

    let run_config = RunConfig {
        is_event_driven: config.is_event_driven,
        acceptors_send_learns: false, // TODO: Hardcoded
        prepare_timeout: 1000,        // TODO: Hardcoded to 1 sec for now.
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

    // Create the PaxosService for gRPC
    let paxos_service = PaxosService {
        client_seq: Arc::new(AtomicU32::new(0)), // TODO: Make this user specific and properly handled
        paxos_wasmtime: paxos_wasmtime.clone(), // TODO: Technically bad practice to make multiple copies of an Arc
    };

    // Wrap the FailureService in an Arc as well
    let failure_service = Arc::new(FailureService {
        paxos_wasmtime: paxos_wasmtime.clone(),
    });

    let run_paxos_service = Arc::new(RunPaxosService {
        paxos_wasmtime: paxos_wasmtime.clone(),
    });

    failure_service.start_heartbeat_sender(
        config.node,
        config.remote_nodes,
        Duration::from_millis(1000),
    ); // Send heartbeats every second

    // TODO: Consider the ratio of failure_check / heartbeat_interval. Currently its 5, but Meling had 10.
    failure_service.start_failure_service(Duration::from_secs(5)); // Check for failures every 5 seconds - Adjust as needed

    // start paxos run loop
    run_paxos_service.start_paxos_run_loop(Duration::from_millis(10));

    // Now run the gRPC server in the foreground
    let addr = config.bind_addr.parse()?;
    info!("gRPC server listening on {}", addr);

    Server::builder()
        .add_service(paxos_proto::paxos_server::PaxosServer::new(paxos_service))
        .serve(addr)
        .await?;

    Ok(())
}
