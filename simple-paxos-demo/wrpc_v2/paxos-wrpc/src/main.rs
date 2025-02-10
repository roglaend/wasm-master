// use std::net::SocketAddr;
// use std::pin::pin;
// use std::sync::Arc;
// use std::time::Duration;

// use anyhow::Context;
// use clap::Parser;
// use tracing::{debug, error, info, warn};
// use wit_bindgen_wrpc::anyhow::Result as _;
// use wit_bindgen_wrpc::tokio::sync::RwLock;
// use wit_bindgen_wrpc::tokio::task::JoinSet;
// use wit_bindgen_wrpc::{tokio, tracing};
// use wrpc_transport::tcp::Client; // TCP client from wrpc_transport
// use wrpc_transport::{ResourceBorrow, ResourceOwn};

// mod bindings {
//     wit_bindgen_wrpc::generate!({
//         world: "paxos-world",
//     });
// }

// use bindings::exports::paxos::wrpc::paxos_coordinator::{
//     Handler, HandlerPaxosCoordinatorResource, PaxosCoordinatorResource,
// };

// use bindings::paxos::wrpc::acceptor as Acceptor;
// use bindings::paxos::wrpc::learner as Learner;
// use bindings::paxos::wrpc::proposer as Proposer;

// /// Command-line arguments for the coordinator.
// #[derive(Parser, Debug)]
// #[command(author, version, about, long_about = None)]
// struct Args {
//     /// Paxos Nodes' addresses (e.g., "127.0.0.1:7001"). Supply multiple times.
//     #[arg(long)]
//     nodes: Vec<String>,
//     /// Time to wait (in seconds) between proposals.
//     #[arg(long, default_value_t = 30)]
//     proposal_interval: u64,
// }

// #[derive(Default)]
// struct MyPaxosCoordinator {
//     // paxos_resources: Arc<RwLock<Vec<MyPaxosCoordinatorResource>>>,
//     paxos_resource: RwLock<MyPaxosCoordinatorResource>,
// }

// #[derive(Default)]
// struct MyPaxosCoordinatorResource {
//     self_node_address: String,
//     node_addresses: Vec<String>,
//     num_nodes: u64,
// }

// impl MyPaxosCoordinator {
//     /// A constructor so that our coordinator can be created from a list of node addresses.
//     fn new(nodes: Vec<String>) -> Self {
//         let resource = MyPaxosCoordinatorResource {
//             self_node_address: String::new(), // Fill in your local address if needed.
//             node_addresses: nodes.clone(),
//             num_nodes: nodes.len() as u64,
//         };
//         MyPaxosCoordinator {
//             paxos_resource: RwLock::new(resource),
//         }
//     }
// }

// // impl<Ctx: Send> Handler<Ctx> for MyPaxosCoordinator {
// //     fn get_nodes_paxos_coordinator_resource(&self, _id: String) -> Result<PaxosCoordinatorResource> {
// //         // Assuming that PaxosCoordinatorResource is defined (via wit-bindgen) as a type alias
// //         // for our RwLock-wrapped resource.
// //         Ok(self.paxos_resource.clone())
// //     }
// // }

// impl<Ctx: Send> HandlerPaxosCoordinatorResource<Ctx> for MyPaxosCoordinatorResource {
//     // pub async fn create_new(&self, self_node: &String, nodes: Vec<String>) -> Self {
//     //     let wrpc = Client::from(self_node);
//     //     let handle = Proposer::ProposerResource::new(&wrpc, (), nodes.len().try_into().unwrap())
//     //         .await
//     //         .context("should exist locally").unwrap();

//     //     for node in nodes
//     //     Self {
//     //         self_node: self_node.clone(),
//     //         node_addresses: nodes,
//     //         proposer_handle: handle,
//     //     }

//     // }

//     //     fn proposer_propose_proxy(&self) {
//     //         todo!()
//     //     }

//     //     fn acceptor_prepare_proxy(&self) {
//     //         todo!()
//     //     }

//     //     fn acceptor_accept_proxy(&self) {}

//     //     // fn new(&self,cx:Ctx,) -> impl ::core::future::Future<Output =  ::wit_bindgen_wrpc::anyhow::Result<::wit_bindgen_wrpc::wrpc_transport::ResourceOwn<PaxosCoordinatorResource>>> + ::core::marker::Send {
//     //     //     todo!()
//     //     // }
// }

// impl MyPaxosCoordinator {
//     async fn create_nodes_paxos_coordinator_resource(
//         &self,
//         self_node: &String,
//         nodes: Vec<String>,
//     ) {
//         let new_paxos_resource = MyPaxosCoordinatorResource {
//             self_node_address: self_node.clone(),
//             node_addresses: nodes.clone(),
//             num_nodes: nodes.len() as u64,
//         };

//         let mut resource_guard = self.paxos_resource.write().await;
//         *resource_guard = new_paxos_resource;
//     }

//     /// Runs a full Paxos round:
//     ///
//     /// 1. Prepare phase: call `prepare(proposal-id)` on all acceptors.
//     /// 2. Accept phase: if a majority respond positively, call `accept(proposal-id, value)`.
//     /// 3. Notify learners: call `learn(value)` on all learners.
//     ///
//     /// Returns Ok(true) if the round is successful.
//     pub async fn propose_value(&self, proposal_id: u64, value: String) -> anyhow::Result<bool> {
//         // Determine majority.
//         let paxos_resource = self.paxos_resource.read().await;
//         let total_acceptors = paxos_resource.num_nodes;
//         let majority = (total_acceptors / 2) + 1;

//         // --- Phase 1: PREPARE ---
//         info!("Starting PREPARE phase for proposal {}", proposal_id);
//         let mut prepare_tasks = JoinSet::new();
//         // Need to call exported handler function on all acceptors over wrpc
//         for addr in paxos_resource.node_addresses.iter().cloned() {
//             let pid = proposal_id;
//             prepare_tasks.spawn(async move {
//                 let wrpc = Client::from(addr.clone());
//                 // let result = Acceptor::AcceptorResource::prepare(
//                 //     &wrpc,
//                 //     (),
//                 //     // ResourceBorrow::from(Bytes::copy_from_slice(&addr.clone()))
//                 //     pid,
//             });
//         }

//         let mut prepare_ok = 0;
//         let mut responded = 0;
//         while let Some(res) = prepare_tasks.join_next().await {
//             responded += 1;
//             match res {
//                 Ok(Ok(true)) => prepare_ok += 1,
//                 Ok(Ok(false)) => warn!("An acceptor rejected prepare"),
//                 Ok(Err(err)) => warn!("Error during prepare: {:?}", err),
//                 Err(err) => warn!("Task join error: {:?}", err),
//             }
//         }
//         info!(
//             "PREPARE phase: {} acceptors responded positively out of {}",
//             prepare_ok, responded
//         );
//         if prepare_ok < majority {
//             warn!("PREPARE phase failed to achieve majority");
//             return Ok(false);
//         }

//         // --- Phase 2: ACCEPT ---
//         info!("Starting ACCEPT phase for proposal {}", proposal_id);
//         let mut accept_tasks = JoinSet::new();
//         for addr in paxos_resource.node_addresses.iter().cloned() {
//             let pid = proposal_id;
//             let val = value.clone();
//             accept_tasks.spawn(async move {
//                 let client = Client::from(addr.clone());
//                 let res = Acceptor::AcceptorResource::new(&client, ())
//                     .await
//                     .with_context(|| format!("failed to create acceptor resource on {addr}"))?;
//                 let accepted =
//                     Acceptor::AcceptorResource::accept(&client, (), &res.as_borrow(), pid, val)
//                         .await
//                         .with_context(|| format!("accept RPC failed on {addr}"))?;
//                 Ok::<bool, anyhow::Error>(accepted)
//             });
//         }

//         let mut accept_ok = 0;
//         responded = 0;
//         while let Some(res) = accept_tasks.join_next().await {
//             responded += 1;
//             match res {
//                 Ok(Ok(true)) => accept_ok += 1,
//                 Ok(Ok(false)) => warn!("An acceptor rejected the proposal"),
//                 Ok(Err(err)) => warn!("Error during accept phase: {:?}", err),
//                 Err(err) => warn!("Task join error: {:?}", err),
//             }
//         }
//         info!(
//             "ACCEPT phase: {} acceptors accepted out of {}",
//             accept_ok, responded
//         );
//         if accept_ok < majority {
//             warn!("ACCEPT phase failed to achieve majority");
//             return Ok(false);
//         }

//         // --- Phase 3: LEARN ---
//         info!("Starting LEARN phase: notifying learners");
//         // let mut learner_tasks = JoinSet::new();
//         // for addr in paxos_resource.node_addresses.iter().cloned() {
//         //     let val = value.clone();
//         //     learner_tasks.spawn(async move {
//         //         let client = Client::from(addr.clone());
//         //         let res = Learner::LearnerResource::new(&client, ())
//         //             .await
//         //             .with_context(|| format!("failed to create learner resource on {addr}"))?;
//         //         Learner::LearnerResource::learn(&client, (), &res.as_borrow(), val)
//         //             .await
//         //             .with_context(|| format!("learn RPC failed on {addr}"))
//         //     });
//         // }

//         // Wait for all learner notifications (we log but do not require every learner to succeed).
//         while let Some(res) = learner_tasks.join_next().await {
//             if let Err(err) = res {
//                 warn!("Learner task join error: {:?}", err);
//             } else if let Err(err) = res.unwrap() {
//                 warn!("Learner notification error: {:?}", err);
//             }
//         }
//         info!("LEARN phase complete");

//         Ok(true)
//     }
// }

// #[tokio::main]
// async fn main() -> anyhow::Result<()> {
//     // Initialize logging.
//     tracing_subscriber::fmt::init();

//     // Parse command-line arguments.
//     let args = Args::parse();
//     if args.nodes.is_empty() {
//         anyhow::bail!("At least one --node address must be specified");
//     }
//     info!("Coordinator starting with nodes: {:?}", args.nodes);

//     let coordinator = Arc::new(MyPaxosCoordinator::new(args.nodes));

//     for i in 0..3{
//         let proposal_id = &i;
//         let proposal_value = format!("Value for proposal {}", proposal_id);
//         match coordinator.propose_value(proposal_id, proposal_value).await {
//             Ok(true) => info!("Proposal {} succeeded", proposal_id),
//             Ok(false) => warn!("Proposal {} was rejected", proposal_id),
//             Err(err) => error!("Error during proposal {}: {:?}", proposal_id, err),
//         };
//     }

//     info!("Coordinator shutting down");
//     Ok(())

// }

// // #[tokio::main]
// // async fn main() -> anyhow::Result<()> {
//     // Initialize logging.
//     // tracing_subscriber::fmt::init();

//     // // Parse command-line arguments.
//     // let args = Args::parse();
//     // if args.nodes.is_empty() {
//     //     anyhow::bail!("At least one --node address must be specified");
//     // }
//     // info!("Coordinator starting with nodes: {:?}", args.nodes);

//     // let coordinator = Arc::new(MyPaxosCoordinator::new(args.nodes));

//     // // Create a JoinSet for our proposal tasks.
//     // let mut tasks = JoinSet::new();

//     // Create a shutdown signal (Ctrl+C).
//     // let shutdown_signal = signal::ctrl_c().fuse();
//     // tokio::pin!(shutdown_signal);

//     //     // Run proposals periodically.
//     //     let proposal_interval = Duration::from_secs(args.proposal_interval);
//     //     let mut proposal_timer = tokio::time::interval(proposal_interval);
//     //     // Fire the timer immediately.
//     //     proposal_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

//     //     loop {
//     //         select! {
//     //             _ = proposal_timer.tick() => {
//     //                 // For illustration, we generate a new proposal id each time.
//     //                 let proposal_id = rand::random::<u64>();
//     //                 let value = format!("Value for proposal {proposal_id}");
//     //                 let coordinator = Arc::clone(&coordinator);
//     //                 tasks.spawn(async move {
//     //                     match coordinator.propose_value(proposal_id, value).await {
//     //                         Ok(true) => info!("Proposal {} succeeded", proposal_id),
//     //                         Ok(false) => warn!("Proposal {} was rejected", proposal_id),
//     //                         Err(err) => error!("Error during proposal {}: {:?}", proposal_id, err),
//     //                     }
//     //                 });
//     //             }
//     //             // Monitor for completed tasks.
//     //             Some(task_res) = tasks.join_next() => {
//     //                 if let Err(err) = task_res {
//     //                     error!("Task join error: {:?}", err);
//     //                 }
//     //             }
//     //             // Exit if a shutdown signal is received.
//     //             _ = &mut shutdown_signal => {
//     //                 info!("Shutdown signal received. Waiting for running tasks to complete...");
//     //                 // Abort any pending proposal tasks.
//     //                 while let Some(task) = tasks.join_next().await {
//     //                     if let Err(err) = task {
//     //                         error!("Error in task during shutdown: {:?}", err);
//     //                     }
//     //                 }
//     //                 break;
//     //             }
//     //         }
//     //     }

//     info!("Coordinator shutting down");
//     Ok(())
// }

use anyhow::Context;
use std::time::Duration;
use tracing::{error, info, warn};
use wit_bindgen_wrpc::tokio::task::JoinSet;
use wit_bindgen_wrpc::{tokio, tracing};
use wrpc_transport::tcp::Client;
use wrpc_transport::{ResourceBorrow, ResourceOwn};

mod bindings {
    // Generate bindings for our Paxos world (which imports proposer, acceptor, and learner).
    wit_bindgen_wrpc::generate!({
        world: "paxos-world",
    });
}

// Bring the generated modules into scope.
use bindings::paxos::wrpc::acceptor as Acceptor;
use bindings::paxos::wrpc::learner as Learner;
use bindings::paxos::wrpc::proposer as Proposer;

// Example pure rust host. Might be able to replicate this logic in the wasm component attempt above
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging.
    tracing_subscriber::fmt::init();

    // Define the addresses for each service.
    // (Adjust these addresses to match your environment.)
    let proposer_addr = "127.0.0.1:7000";
    let acceptor_addrs = vec!["127.0.0.1:7001", "127.0.0.1:7002", "127.0.0.1:7005"];
    let learner_addrs = vec!["127.0.0.1:7003", "127.0.0.1:7004"];

    // ----------- PROPOSER -----------
    // Create a TCP client for the proposer.
    let proposer_client = Client::from(proposer_addr);
    // Create a proposer resource. We pass in the number of acceptors.
    let num_acceptors = acceptor_addrs.len() as u64;
    let proposer_resource = Proposer::ProposerResource::new(&proposer_client, (), num_acceptors)
        .await
        .context("failed to create proposer resource")?;

    // (Optionally) Retrieve and log the initial proposer state.
    let state =
        Proposer::ProposerResource::get_state(&proposer_client, (), &proposer_resource.as_borrow())
            .await
            .context("failed to get proposer state")?;
    info!("Initial proposer state: {:?}", state);

    // We'll run three rounds.
    for round in 1..=3 {
        info!("========== Starting Paxos Round {} ==========", round);
        // For each round, we use a different proposal ID and value.
        // (Here, we're simply using the round number as the proposal ID.)
        let proposal_id = round;
        let proposal_value = format!("example value round {}", round);

        // ----------- PREPARE PHASE (ACCEPTORS) -----------
        // Call prepare(proposal-id) on all acceptors.
        let mut prepare_tasks = JoinSet::new();
        for addr in &acceptor_addrs {
            let addr = addr.to_string();
            prepare_tasks.spawn(async move {
                let client = Client::from(addr.clone());
                let acceptor_resource = Acceptor::AcceptorResource::new(&client, ())
                    .await
                    .with_context(|| format!("failed to create acceptor resource on {addr}"))?;
                let prep_result = Acceptor::AcceptorResource::prepare(
                    &client,
                    (),
                    &acceptor_resource.as_borrow(),
                    proposal_id,
                )
                .await
                .with_context(|| format!("prepare RPC failed on {addr}"))?;
                Ok::<bool, anyhow::Error>(prep_result)
            });
        }
        let mut prepare_ok = 0;
        while let Some(res) = prepare_tasks.join_next().await {
            match res {
                Ok(Ok(true)) => prepare_ok += 1,
                Ok(Ok(false)) => warn!("An acceptor rejected the prepare"),
                Ok(Err(err)) => warn!("Error during prepare: {:?}", err),
                Err(err) => warn!("Task join error: {:?}", err),
            }
        }
        info!(
            "Round {}: PREPARE phase: {} acceptors accepted",
            round, prepare_ok
        );
        // If a majority did not accept, skip to the next round.
        let majority = (acceptor_addrs.len() / 2) + 1;
        if prepare_ok < majority {
            warn!(
                "Round {}: Prepare phase did not reach majority ({} required). Skipping round.",
                round, majority
            );
            continue;
        }

        // ----------- ACCEPT PHASE (ACCEPTORS) -----------
        // Call accept(proposal-id, value) on all acceptors.
        let mut accept_tasks = JoinSet::new();
        for addr in &acceptor_addrs {
            let addr = addr.to_string();
            let value = proposal_value.clone();
            accept_tasks.spawn(async move {
                let client = Client::from(addr.clone());
                let acceptor_resource = Acceptor::AcceptorResource::new(&client, ())
                    .await
                    .with_context(|| format!("failed to create acceptor resource on {addr}"))?;
                let accepted = Acceptor::AcceptorResource::accept(
                    &client,
                    (),
                    &acceptor_resource.as_borrow(),
                    proposal_id,
                    &value,
                )
                .await
                .with_context(|| format!("accept RPC failed on {addr}"))?;
                Ok::<bool, anyhow::Error>(accepted)
            });
        }
        let mut accept_ok = 0;
        while let Some(res) = accept_tasks.join_next().await {
            match res {
                Ok(Ok(true)) => accept_ok += 1,
                Ok(Ok(false)) => warn!("An acceptor rejected the proposal"),
                Ok(Err(err)) => warn!("Error during accept phase: {:?}", err),
                Err(err) => warn!("Task join error: {:?}", err),
            }
        }
        info!(
            "Round {}: ACCEPT phase: {} acceptors accepted",
            round, accept_ok
        );
        if accept_ok < majority {
            warn!(
                "Round {}: Accept phase did not reach majority. Skipping round.",
                round
            );
            continue;
        }

        // ----------- LEARN PHASE (LEARNERS) -----------
        // Notify each learner about the new proposal value.
        let mut learner_tasks = JoinSet::new();
        for addr in &learner_addrs {
            let addr = addr.to_string();
            let value = proposal_value.clone();
            learner_tasks.spawn(async move {
                let client = Client::from(addr.clone());
                let learner_resource = Learner::LearnerResource::new(&client, ())
                    .await
                    .with_context(|| format!("failed to create learner resource on {addr}"))?;
                Learner::LearnerResource::learn(&client, (), &learner_resource.as_borrow(), &value)
                    .await
                    .with_context(|| format!("learn RPC failed on {addr}"))
            });
        }
        // Log any learner errors (but we do not abort the round for learner errors).
        while let Some(res) = learner_tasks.join_next().await {
            if let Err(err) = res {
                warn!("Learner task error: {:?}", err);
            }
        }
        info!(
            "Round {}: LEARN phase complete. Paxos round successful.",
            round
        );

        // (Optional) Wait some time before starting the next round.
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // (Optionally) Retrieve and log the final proposer state.
    let final_state =
        Proposer::ProposerResource::get_state(&proposer_client, (), &proposer_resource.as_borrow())
            .await
            .context("failed to get proposer state")?;
    info!("Final proposer state after rounds: {:?}", final_state);

    Ok(())
}
