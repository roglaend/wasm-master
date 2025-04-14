mod acceptor;
mod learner;
mod proposer;

mod standalone_config;

use proposer::failure_service::FailureService as ProposerFailureService;
use proposer::grpc_service::PaxosService as ProposerPaxosService;
use proposer::paxos_bindings::paxos::default::paxos_types::RunConfig as ProposerRunConfig;
use proposer::paxos_wasm::PaxosWasmtime as ProposerPaxosWasmtime;
use proposer::run_paxos_service::RunPaxosService as ProposerRunPaxosService;

use acceptor::failure_service::FailureService as AcceptorFailureService;
use acceptor::grpc_service::PaxosService as AcceptorPaxosService;
use acceptor::paxos_bindings::paxos::default::paxos_types::RunConfig as AcceptorRunConfig;
use acceptor::paxos_wasm::PaxosWasmtime as AcceptorPaxosWasmtime;
use acceptor::run_paxos_service::RunPaxosService as AcceptorRunPaxosService;

use learner::failure_service::FailureService as LearnerFailureService;
use learner::grpc_service::PaxosService as LearnerPaxosService;
use learner::paxos_bindings::paxos::default::paxos_types::RunConfig as LearnerRunConfig;
use learner::paxos_wasm::PaxosWasmtime as LearnerPaxosWasmtime;
use learner::run_paxos_service::RunPaxosService as LearnerRunPaxosService;

use clap::Parser;
use proto::paxos_proto;
use standalone_config::Config;
use std::convert::TryInto;
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

    if config.node.role == 1 {
        let node: proposer::paxos_bindings::paxos::default::paxos_types::Node =
            config.node.clone().try_into()?;
        let nodes: Vec<proposer::paxos_bindings::paxos::default::paxos_types::Node> = config
            .remote_nodes
            .clone()
            .iter()
            .map(|node| node.clone().try_into())
            .collect::<Result<Vec<_>, _>>()?;
        let run_config = ProposerRunConfig {
            is_event_driven: config.is_event_driven,
            acceptors_send_learns: false,
            prepare_timeout: 1000,
        };

        let paxos_wasmtime = Arc::new(
            ProposerPaxosWasmtime::new(node.clone(), nodes.clone(), is_leader, run_config).await?,
        );
        let paxos_service = ProposerPaxosService {
            client_seq: Arc::new(AtomicU32::new(0)), // TODO: Make this user specific and properly handled
            paxos_wasmtime: paxos_wasmtime.clone(), // TODO: Technically bad practice to make multiple copies of an Arc
        };

        // Wrap the FailureService in an Arc as well
        let failure_service = Arc::new(ProposerFailureService {
            paxos_wasmtime: paxos_wasmtime.clone(),
        });

        let run_paxos_service = Arc::new(ProposerRunPaxosService {
            paxos_wasmtime: paxos_wasmtime.clone(),
        });

        failure_service.start_heartbeat_sender(
            node.clone(),
            nodes.clone(),
            Duration::from_millis(5000),
        ); // Send heartbeats every second

        // TODO: Consider the ratio of failure_check / heartbeat_interval. Currently its 5, but Meling had 10.
        failure_service.start_failure_service(Duration::from_secs(5)); // Check for failures every 5 seconds - Adjust as needed

        // start paxos run loop
        run_paxos_service.start_paxos_run_loop(Duration::from_millis(10));

        // run_paxos_service.start_paxos_run_loop(Duration::from_millis(1000));

        // Now run the gRPC server in the foreground
        let addr = config.bind_addr.parse()?;
        info!("gRPC server listening on {}", addr);

        Server::builder()
            .add_service(paxos_proto::paxos_server::PaxosServer::new(paxos_service))
            .serve(addr)
            .await?;

        Ok(())
    } else if config.node.role == 2 {
        let node: acceptor::paxos_bindings::paxos::default::paxos_types::Node =
            config.node.clone().try_into()?;
        let nodes: Vec<acceptor::paxos_bindings::paxos::default::paxos_types::Node> = config
            .remote_nodes
            .clone()
            .iter()
            .map(|node| node.clone().try_into())
            .collect::<Result<Vec<_>, _>>()?;
        let run_config = AcceptorRunConfig {
            is_event_driven: config.is_event_driven,
            acceptors_send_learns: false,
            prepare_timeout: 1000,
        };

        let paxos_wasmtime = Arc::new(
            AcceptorPaxosWasmtime::new(node.clone(), nodes.clone(), is_leader, run_config).await?,
        );
        let paxos_service = AcceptorPaxosService {
            client_seq: Arc::new(AtomicU32::new(0)), // TODO: Make this user specific and properly handled
            paxos_wasmtime: paxos_wasmtime.clone(), // TODO: Technically bad practice to make multiple copies of an Arc
        };

        // Wrap the FailureService in an Arc as well
        let failure_service = Arc::new(AcceptorFailureService {
            paxos_wasmtime: paxos_wasmtime.clone(),
        });

        let run_paxos_service = Arc::new(AcceptorRunPaxosService {
            paxos_wasmtime: paxos_wasmtime.clone(),
        });

        failure_service.start_heartbeat_sender(
            node.clone(),
            nodes.clone(),
            Duration::from_millis(5000),
        ); // Send heartbeats every second

        // TODO: Consider the ratio of failure_check / heartbeat_interval. Currently its 5, but Meling had 10.
        failure_service.start_failure_service(Duration::from_secs(15)); // Check for failures every 5 seconds - Adjust as needed

        // start paxos run loop
        // run_paxos_service.start_paxos_run_loop(Duration::from_millis(10));

        // run_paxos_service.start_paxos_run_loop(Duration::from_millis(1000));

        // Now run the gRPC server in the foreground
        let addr = config.bind_addr.parse()?;
        info!("gRPC server listening on {}", addr);

        Server::builder()
            .add_service(paxos_proto::paxos_server::PaxosServer::new(paxos_service))
            .serve(addr)
            .await?;
        Ok(())
    } else if config.node.role == 3 {
        let node: learner::paxos_bindings::paxos::default::paxos_types::Node =
            config.node.clone().try_into()?;
        let nodes: Vec<learner::paxos_bindings::paxos::default::paxos_types::Node> = config
            .remote_nodes
            .clone()
            .iter()
            .map(|node| node.clone().try_into())
            .collect::<Result<Vec<_>, _>>()?;
        let run_config = LearnerRunConfig {
            is_event_driven: config.is_event_driven,
            acceptors_send_learns: false,
            prepare_timeout: 1000,
        };

        let paxos_wasmtime = Arc::new(
            LearnerPaxosWasmtime::new(node.clone(), nodes.clone(), is_leader, run_config).await?,
        );
        let paxos_service = LearnerPaxosService {
            client_seq: Arc::new(AtomicU32::new(0)), // TODO: Make this user specific and properly handled
            paxos_wasmtime: paxos_wasmtime.clone(), // TODO: Technically bad practice to make multiple copies of an Arc
        };

        // Wrap the FailureService in an Arc as well
        let failure_service = Arc::new(LearnerFailureService {
            paxos_wasmtime: paxos_wasmtime.clone(),
        });

        let run_paxos_service = Arc::new(LearnerRunPaxosService {
            paxos_wasmtime: paxos_wasmtime.clone(),
        });

        failure_service.start_heartbeat_sender(
            node.clone(),
            nodes.clone(),
            Duration::from_millis(5000),
        ); // Send heartbeats every second

        // TODO: Consider the ratio of failure_check / heartbeat_interval. Currently its 5, but Meling had 10.
        failure_service.start_failure_service(Duration::from_secs(5)); // Check for failures every 5 seconds - Adjust as needed

        // start paxos run loop
        run_paxos_service.start_paxos_run_loop(Duration::from_millis(50));

        // run_paxos_service.start_paxos_run_loop(Duration::from_millis(1000));

        // Now run the gRPC server in the foreground
        let addr = config.bind_addr.parse()?;
        info!("gRPC server listening on {}", addr);

        Server::builder()
            .add_service(paxos_proto::paxos_server::PaxosServer::new(paxos_service))
            .serve(addr)
            .await?;
        Ok(())
    } else {
        Ok(())
    }
}
