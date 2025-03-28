#![allow(unsafe_op_in_unsafe_fn)]

use bindings::paxos::default::network::{NetworkMessage, NetworkResponse};
use bindings::paxos::default::proposer::ClientProposal;
use core::panic;
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::{format, Debug};
use std::sync::{Arc, Mutex};

pub mod bindings {
    wit_bindgen::generate!( {
        path: "../../shared/wit/paxos.wit",
        world: "paxos-world",
    });
}

bindings::export!(MyPaxosCoordinator with_types_in bindings);

use bindings::exports::paxos::default::paxos_coordinator::{
    AcceptedResult, ElectionResult, Guest as CoordinatorGuest, GuestPaxosCoordinatorResource,
    LearnResult, PaxosState, PromiseResult,
};
use bindings::paxos::default::{acceptor, kv_store, learner, logger, network, proposer, failure_detector};

pub struct MyPaxosCoordinator;

impl CoordinatorGuest for MyPaxosCoordinator {
    type PaxosCoordinatorResource = MyPaxosCoordinatorResource;
}

pub struct MyPaxosCoordinatorResource {
    // The list of nodes in the cluster.
    nodes: Vec<network::Node>,

    // The component resources.
    proposer: Arc<proposer::ProposerResource>,
    acceptor: Arc<acceptor::AcceptorResource>,
    learner: Arc<learner::LearnerResource>,
    kv_store: Arc<kv_store::KvStoreResource>,
    failure_detector: Arc<failure_detector::FailureDetectorResource>,
    // A key for storing committed Paxos values in the key/value store.
    paxos_key: String,



    // heartbeats: Arc<Mutex<HashMap<u64, u64>>>,
    // node_id: u64,
    // leader_id: Cell<u64>,
    // election_in_progress: Cell<bool>

}

impl MyPaxosCoordinatorResource {
    /// Returns the required quorum (here, a majority).
    fn get_quorum(&self) -> u64 {
        (self.nodes.len() as u64 / 2) + 1
    }

    /// Combines local votes with remote responses to check if a quorum is met.
    fn combined_quorum(&self, local_votes: u64, responses: &[network::NetworkResponse]) -> bool {
        let remote_success_count = responses
            .iter()
            .filter(|r| r.status == network::StatusKind::Success)
            .count() as u64;
        (local_votes + remote_success_count) >= self.get_quorum()
    }

    /// Determines the proposal value:
    /// If any acceptor (via the promise result) already has accepted a value,
    /// that value must be re-proposed. Otherwise, the client's value is used.
    fn choose_proposal_value(&self, promise: PromiseResult, client_value: &str) -> String {
        if let Some(ref accepted) = promise.accepted_value {
            logger::log_info(&format!(
                "Using previously accepted value from promise: {}",
                accepted
            ));
            accepted.clone()
        } else {
            logger::log_info(&format!(
                "No accepted value found; using client value: {}",
                client_value
            ));
            client_value.to_string()
        }
    }

    /// Helper that builds a NetworkResponse from a message kind and a boolean result.
    fn build_response(&self, kind: network::NetworkMessageKind, result: bool) -> NetworkResponse {
        NetworkResponse {
            kind,
            status: if result {
                network::StatusKind::Success
            } else {
                network::StatusKind::Failure
            },
        }
    }

    /// Generic helper to process an optional payload.
    /// It takes the message kind, an Option-wrapped payload, and a handler that returns a bool.
    fn process_payload<T, F>(
        &self,
        kind: network::NetworkMessageKind,
        payload_option: Option<T>,
        handler: F,
    ) -> NetworkResponse
    where
        T: Debug,
        F: FnOnce(T) -> bool,
    {
        match payload_option {
            Some(p) => {
                logger::log_info(&format!("Processing payload for kind {:?}: {:?}", kind, p));
                let result = handler(p);
                self.build_response(kind, result)
            }
            None => {
                logger::log_warn(&format!("Message of kind {:?} missing payload", kind));
                self.build_response(kind, false)
            }
        }
    }
}

impl GuestPaxosCoordinatorResource for MyPaxosCoordinatorResource {
    /// Creates a new coordinator resource.
    fn new(node_id: u64, nodes: Vec<network::Node>) -> Self {
        let num_nodes = nodes.len() as u64;
        let proposer = Arc::new(proposer::ProposerResource::new(
            node_id,
            num_nodes,
            node_id == 4, //Leader detector should decide this after startup
        ));
        let acceptor = Arc::new(acceptor::AcceptorResource::new());
        let learner = Arc::new(learner::LearnerResource::new());
        let kv_store = Arc::new(kv_store::KvStoreResource::new());

        let failure_detector = Arc::new(failure_detector::FailureDetectorResource::new(node_id, &nodes, 10) );

        Self {
            nodes,
            proposer,
            acceptor,
            learner,
            kv_store,
            paxos_key: "paxos_values".to_string(), // TODO: Change the kv-store to save paxos values for slots instead of using the "history"
            // heartbeats: Arc::new(Mutex::new(HashMap::new())),
            // leader_id: Cell::new(4),
            // node_id,
            // election_in_progress: Cell::new(false),
            failure_detector,
        }
    }

    /// Phase 1: Prepare Phase.
    ///
    /// Performs a local prepare (using the acceptor) for the given slot and ballot,
    /// then broadcasts a prepare message to remote nodes.
    /// Returns a PromiseResult including the ballot and (if any) accepted value.
    fn prepare_phase(&self, ballot: u64, slot: u64) -> PromiseResult {
        // Local prepare: note that the updated acceptor now expects both slot and ballot.
        let local_prepared = self.acceptor.prepare(slot, ballot);
        if !local_prepared {
            logger::log_error(&format!(
                "Prepare phase: local prepare failed for slot {} with ballot {}.",
                slot, ballot
            ));
            panic!("Prepare phase failure");
        }
        // In a more complete system we would check local acceptance state here.
        let required_quorum = self.get_quorum();
        let local_promise = PromiseResult {
            ballot,
            accepted_ballot: 0,   // For now, assume no accepted value locally.
            accepted_value: None, // In a complete implementation, inspect the local acceptor state.
            quorum: required_quorum,
        };

        // Build and broadcast the prepare message.
        let prepare_payload = network::PreparePayload { slot, ballot };
        let message = network::NetworkMessage {
            kind: network::NetworkMessageKind::Prepare,
            payload: network::MessagePayload::Prepare(prepare_payload),
        };

        // Broadcast to remote nodes (this is a synchronous call for now).
        let responses = network::send_message(&self.nodes, &message);
        if !self.combined_quorum(1, &responses) {
            logger::log_error("Prepare phase: remote quorum check failed.");
            panic!("Prepare phase quorum failure");
        }
        local_promise // TODO: Take into account all promises
    }

    /// Phase 2: Accept Phase.
    ///
    /// Executes local acceptance (via the acceptor) and broadcasts an accept message.
    /// Returns an AcceptedResult if a quorum is reached.
    fn accept_phase(&self, proposal_value: String, slot: u64, ballot: u64) -> AcceptedResult {
        // Create an accepted-entry for this proposal.
        let accepted_entry = acceptor::AcceptedEntry {
            slot,
            ballot,
            value: proposal_value.clone(),
        };
        let local_accepted = self.acceptor.accept(&accepted_entry);
        if !local_accepted {
            logger::log_error(&format!(
                "Accept phase: local acceptance failed for slot {}.",
                slot
            ));
            panic!("Local acceptance failure");
        }
        let local_votes = 1; // Local acceptance counts as one vote.

        // Build and broadcast the accept message.
        let accept_payload = network::AcceptPayload {
            slot,
            ballot,
            proposal: proposal_value.clone(),
        };
        let message = network::NetworkMessage {
            kind: network::NetworkMessageKind::Accept,
            payload: network::MessagePayload::Accept(accept_payload),
        };
        let responses = network::send_message(&self.nodes, &message);
        if !self.combined_quorum(local_votes, &responses) {
            logger::log_error(&format!(
                "Accept phase: combined quorum not reached for slot {}.",
                slot
            ));
            panic!("Accept phase quorum failure");
        }
        AcceptedResult {
            accepted_count: local_votes
                + responses
                    .iter()
                    .filter(|r| r.status == network::StatusKind::Success)
                    .count() as u64,
            quorum: self.get_quorum(),
        }
    }

    /// Phase 3: Commit Phase.
    ///
    /// Commits the proposal by updating the learner and key‑value store, then broadcasting a commit message.
    fn commit_phase(&self, slot: u64) -> Option<LearnResult> {
        // Retrieve the last proposal from the proposer.
        let state = self.proposer.get_state(); // TODO: Have proper functions to get explicit state
        if let Some(prop) = state.last_proposal {
            // Update the learner with the learned value.
            self.learner
                .learn(prop.slot, &prop.client_proposal.value.clone());
            // Persist the committed value in the key/value store.
            self.kv_store
                .set(&self.paxos_key, &prop.client_proposal.value);
            // Build and broadcast the commit message.
            let commit_payload = network::CommitPayload {
                slot,
                value: prop.client_proposal.value.clone(),
            };
            let message = network::NetworkMessage {
                kind: network::NetworkMessageKind::Commit,
                payload: network::MessagePayload::Commit(commit_payload),
            };
            let _ = network::send_message(&self.nodes, &message);
            Some(LearnResult {
                learned_value: prop.client_proposal.value,
                quorum: self.get_quorum(),
            })
        } else {
            None
        }
    }

    /// Retrieves the current learned value from the learner.
    fn get_learned_value(&self) -> Option<String> {
        let state = self.learner.get_state();
        state.learned.first().map(|entry| entry.value.clone())
    }

    /// Orchestrates the entire Paxos protocol.
    ///
    /// 1. The proposer creates a proposal.
    /// 2. The coordinator runs the prepare phase.
    /// 3. It then chooses the proposal value (if an accepted value exists, it reuses it).
    /// 4. The accept phase is executed.
    /// 5. Finally, the commit phase is run.
    fn run_paxos(&self, client_value: String) -> bool {
        // Step 1: Propose a new value.
        let client_proposal = ClientProposal {
            value: client_value,
        };
        let pr: proposer::ProposeResult = self.proposer.propose(&client_proposal);
        if !pr.accepted {
            logger::log_warn(&format!(
                "Proposer rejected the proposal (likely not leader)."
            ));
            return false;
        }
        // TODO: Move this logic to another function
        let proposal = pr.proposal;
        let ballot = proposal.ballot; // Leader's ballot.
        let slot = proposal.slot; // Instance of Paxos.

        // Step 2: Prepare Phase.
        let promise = self.prepare_phase(ballot, slot);

        // Step 3: Determine the value to propose.
        let proposal_value = self.choose_proposal_value(promise, &client_proposal.value);

        // Step 4: Accept Phase.
        let _ = self.accept_phase(proposal_value.clone(), slot, ballot);

        // Step 5: Commit Phase.
        let learn_result = self.commit_phase(slot);
        if let Some(lr) = learn_result {
            if lr.learned_value.is_empty() {
                logger::log_error(&format!("Commit phase failed: learned value is empty."));
                return false;
            }
            return true;
        } else {
            return false;
        }
    }

    // Testing of fire and forget
    fn client_handle_test(&self, client_value: String) -> bool{
        let client_proposal = ClientProposal {
            value: client_value,
        };

        let pr: proposer::ProposeResult = self.proposer.propose(&client_proposal);

        let prepare_payload = network::PreparePayload { slot: pr.proposal.slot, ballot: pr.proposal.ballot };
        let message = network::NetworkMessage {
            kind: network::NetworkMessageKind::Prepare,
            payload: network::MessagePayload::Prepare(prepare_payload),
        };

        println!("Sending prepare message at time: {:?}", wasi::clocks::monotonic_clock::now());
        network::send_message_forget(&self.nodes, &message);
        println!("Sent prepare message at time: {:?}", wasi::clocks::monotonic_clock::now());
        true
    }

    /// Handles the message from a remote coordinator.
    /// This function pattern-matches on the incoming NetworkMessage and calls the
    /// appropriate Paxos logic (e.g. on the acceptor, learner, or KV store),
    /// then returns a corresponding NetworkResponse.
    fn handle_message(&self, message: NetworkMessage) -> NetworkResponse {
        logger::log_info(&format!(
            "Coordinator: Received network message: {:?}",
            message
        ));

        match message.kind {
            network::NetworkMessageKind::Prepare => {
                // For a prepare message, we expect a PreparePayload.
                if let network::MessagePayload::Prepare(payload) = message.payload {
                    self.process_payload(
                        network::NetworkMessageKind::Prepare,
                        Some(payload),
                        |prep: network::PreparePayload| {
                            logger::log_info(&format!(
                                "Handling PREPARE: slot={}, ballot={}",
                                prep.slot, prep.ballot,
                            ));
                            self.acceptor.prepare(prep.slot, prep.ballot)
                        },
                    )
                } else {
                    self.build_response(network::NetworkMessageKind::Prepare, false)
                }
            }

            network::NetworkMessageKind::Promise => {
                // For a promise message, we expect a PromisePayload.
                if let network::MessagePayload::Promise(payload) = message.payload {
                    self.process_payload(
                        network::NetworkMessageKind::Promise,
                        Some(payload),
                        |prom: network::PromisePayload| {
                            logger::log_info(&format!(
                                "Handling PROMISE: slot={}, ballot={}, accepted_ballot={}, accepted_value={:?}",
                                prom.slot, prom.ballot, prom.accepted_ballot, prom.accepted_value)
                            );
                            // TODO: Never used?
                            true
                        },
                    )
                } else {
                    self.build_response(network::NetworkMessageKind::Promise, false)
                }
            }

            network::NetworkMessageKind::Accept => {
                // For an accept message, we expect an AcceptPayload.
                if let network::MessagePayload::Accept(payload) = message.payload {
                    self.process_payload(
                        network::NetworkMessageKind::Accept,
                        Some(payload),
                        |acc: network::AcceptPayload| {
                            logger::log_info(&format!(
                                "Handling ACCEPT: slot={}, ballot={}, proposal={}",
                                acc.slot, acc.ballot, acc.proposal
                            ));
                            // Create an accepted entry and process the accept request.
                            let entry = acceptor::AcceptedEntry {
                                slot: acc.slot,
                                ballot: acc.ballot,
                                value: acc.proposal,
                            };
                            self.acceptor.accept(&entry)
                        },
                    )
                } else {
                    self.build_response(network::NetworkMessageKind::Accept, false)
                }
            }

            // TODO: This message should be changed to "Promise", but still won't be used due to Promises only being returned by an Accept message etc.
            network::NetworkMessageKind::Accepted => {
                // For an accepted message, we expect an AcceptedPayload.
                if let network::MessagePayload::Accepted(payload) = message.payload {
                    self.process_payload(
                        network::NetworkMessageKind::Accepted,
                        Some(payload),
                        |accd: network::AcceptedPayload| {
                            logger::log_info(&format!(
                                "Handling ACCEPTED: slot={}, ballot={}, accepted={}",
                                accd.slot, accd.ballot, accd.accepted
                            ));
                            // Update internal state as needed.
                            // TODO: Never used?
                            true
                        },
                    )
                } else {
                    self.build_response(network::NetworkMessageKind::Accepted, false)
                }
            }

            network::NetworkMessageKind::Commit => {
                // For a commit message, we expect a CommitPayload.
                if let network::MessagePayload::Commit(payload) = message.payload {
                    self.process_payload(
                        network::NetworkMessageKind::Commit,
                        Some(payload),
                        |comm: network::CommitPayload| {
                            logger::log_info(&format!(
                                "Handling COMMIT: slot={}, value={}",
                                comm.slot, comm.value
                            ));
                            // Update the learner and persist value in the kv-store.
                            self.learner.learn(comm.slot, &comm.value.clone());
                            self.kv_store.set(&self.paxos_key, &comm.value);
                            true
                        },
                    )
                } else {
                    self.build_response(network::NetworkMessageKind::Commit, false)
                }
            }

            network::NetworkMessageKind::Heartbeat => {
                // For a heartbeat message, we expect a HeartbeatPayload.
                if let network::MessagePayload::Heartbeat(payload) = message.payload {
                    self.process_payload(
                        network::NetworkMessageKind::Heartbeat,
                        Some(payload),
                        |hb: network::HeartbeatPayload| {
                            logger::log_info(&format!(
                                "Handling HEARTBEAT: nodeid={}",
                                hb.nodeid,
                            ));
                            self.failure_detector.heartbeat(hb.nodeid);
                            true
                        },
                    )
                } else {
                    self.build_response(network::NetworkMessageKind::Heartbeat, false)
                }
            }

            // OLD ELECTION AND LEADER MESSAGES
            // network::NetworkMessageKind::Election => {
            //     if let network::MessagePayload::Election(payload) = message.payload {
            //         self.process_payload(
            //             network::NetworkMessageKind::Election,
            //             Some(payload),
            //             |el: network::ElectionPayload| {
            //                 logger::log_info(&format!(
            //                     "Handling ELECTION: candidate_id={}",
            //                     el.candidate_id,
            //                 ));
            //                 if self.node_id > el.candidate_id {
            //                     true
            //                 } else {
            //                     false
            //                 }
            //             },
            //         )
            //     } else {
            //         self.build_response(network::NetworkMessageKind::Election, false)
            //     }    
            // }

            // network::NetworkMessageKind::Leader => {
            //     if let network::MessagePayload::Leader(payload) = message.payload {
            //         self.process_payload(
            //             network::NetworkMessageKind::Leader,
            //             Some(payload),
            //             |l: network::LeaderPayload| {
            //                 logger::log_debug(&format!(
            //                     "Handling LEADER CHANGE: leader_id={}",
            //                     l.leader_id,
            //                 ));
            //                 self.leader_id.set(l.leader_id);
            //                 self.election_in_progress.set(false);
            //                 true
            //             },
            //         )
            //     } else {
            //         self.build_response(network::NetworkMessageKind::Heartbeat, false)
            //     }    
            // }
        }
    }

    /// Expose a snapshot of the current Paxos state.
    /// This aggregates the learner state (learned entries) and the key/value store state.
    fn get_state(&self) -> PaxosState {
        let learner_state = self.learner.get_state();
        let kv_state = self.kv_store.get_state();
        PaxosState {
            learned: learner_state.learned,
            kv_state,
        }
    }

    fn elect_leader(&self,) -> ElectionResult {
        todo!()
    }

    fn failure_service(&self) {
        // TODO : What to do with leader change
        // TODO : Also need to handle change in timeout
        let new_lead = self.failure_detector.checker();
        if let Some(leader) = new_lead {
            logger::log_warn(&format!("Leader {} change initiated. New leader", &leader));
        }
    }

    // OLD STALE CHECKER
    // fn stale_checker(&self, threshold: u64) {
    //     // Currently leader does not check for staleness
    //     let leader_id = self.leader_id.get();
    //     if leader_id == self.node_id {
    //         return;
    //     }
    //     let now = wasi::clocks::monotonic_clock::now();
    //     let map = self.heartbeats.lock().unwrap();
    //     let stale_nodes: Vec<u64> = map
    //         .iter()
    //         .filter_map(|(nodeid, timestamp)| {
    //             if now - *timestamp > threshold {
    //                 Some(*nodeid)
    //             } else {
    //                 None
    //             }
    //         })
    //         .collect();
        
    //     // Check if leader is stale 
    //     // Assume this means failure :clownemoji: 
    //     // Start election
    //     if stale_nodes.contains(&leader_id) && !self.election_in_progress.get() {
    //         logger::log_warn(&format!("Leader {} is stale. Triggering leader election.", &leader_id));
    //         let election_payload = network::ElectionPayload {
    //             candidate_id: self.node_id,
    //         };
    //         let message = network::NetworkMessage {
    //             kind: network::NetworkMessageKind::Election,
    //             payload: network::MessagePayload::Election(election_payload),
    //         };

    //         let mut higher_nodes = vec![];
    //         for node in &self.nodes {
    //             if node.id > self.node_id {
    //                 higher_nodes.push(node.clone());
    //             }
    //         }

    //         let responses = network::send_message(&higher_nodes, &message);
    //         // Check atleast one response is success
    //         if responses.iter().any(|r| r.status == network::StatusKind::Success) {
    //             logger::log_debug("Higher node id exist - Waiting for LEADER CHANGE message");
    //             self.election_in_progress.set(true);

    //         } else {
    //             logger::log_debug("No response from higher nodeid - becoming leader");
    //             let leader_payload = network::LeaderPayload {
    //                 leader_id: self.node_id,
    //             };
    //             let message = network::NetworkMessage {
    //                 kind: network::NetworkMessageKind::Leader,
    //                 payload: network::MessagePayload::Leader(leader_payload),
    //             };
    //             //
    //             network::send_message(&self.nodes, &message);

    //             // Update leader id
    //             self.leader_id.set(self.node_id);
    //             self.proposer.become_leader();
    //         }

    //     }
    // }
}
