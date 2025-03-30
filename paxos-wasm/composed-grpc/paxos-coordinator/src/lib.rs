use core::panic;
use std::sync::Arc;

pub mod bindings {
    wit_bindgen::generate!( {
        path: "../../shared/wit",
        world: "paxos-world",
        // additional_derives: [Clone],
    });
}

bindings::export!(MyPaxosCoordinator with_types_in bindings);

use bindings::exports::paxos::default::paxos_coordinator::{
    AcceptResult, ElectionResult, Guest as CoordinatorGuest, GuestPaxosCoordinatorResource,
    LearnResult, PaxosState, PrepareResult,
};
use bindings::paxos::default::network_types::{MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{Ballot, ClientRequest, Learn, Node, Slot, Value};
use bindings::paxos::default::{
    acceptor_agent, kv_store, learner, logger, network, proposer_agent,
};

pub struct MyPaxosCoordinator;

impl CoordinatorGuest for MyPaxosCoordinator {
    type PaxosCoordinatorResource = MyPaxosCoordinatorResource;
}

pub struct MyPaxosCoordinatorResource {
    node: Node,

    // The list of nodes in the cluster.
    nodes: Vec<Node>,

    // Uses the agents
    proposer_agent: Arc<proposer_agent::ProposerAgentResource>,
    acceptor_agent: Arc<acceptor_agent::AcceptorAgentResource>,

    learner: Arc<learner::LearnerResource>,
    kv_store: Arc<kv_store::KvStoreResource>,

    // A key for storing committed Paxos values in the key/value store.
    paxos_key: String,
}

impl MyPaxosCoordinatorResource {
    /// Returns the required quorum (here, a majority).
    fn get_quorum(&self) -> u64 {
        (self.nodes.len() as u64 / 2) + 1
    }
}

impl GuestPaxosCoordinatorResource for MyPaxosCoordinatorResource {
    /// Creates a new coordinator resource.
    fn new(node: Node, nodes: Vec<Node>, is_leader: bool) -> Self {
        let proposer_agent = Arc::new(proposer_agent::ProposerAgentResource::new(
            &node, &nodes, is_leader,
        ));
        let send_learns = false;
        let acceptor_agent = Arc::new(acceptor_agent::AcceptorAgentResource::new(
            &node,
            &nodes,
            send_learns,
        ));
        let learner = Arc::new(learner::LearnerResource::new());
        let kv_store = Arc::new(kv_store::KvStoreResource::new());
        Self {
            node,
            nodes,
            proposer_agent,
            acceptor_agent,
            learner,
            kv_store,
            paxos_key: "paxos_values".to_string(), // TODO: Change the kv-store to save paxos values for slots instead of using the "history"
        }
    }

    /// Orchestrates a full Paxos round by using the proposer agent and local acceptor.
    fn run_paxos(&self, request: ClientRequest) -> bool {
        // Enqueue client request.
        self.proposer_agent.submit_client_request(&request); // TODO: Decouple this from the run_paxos main loop

        // Get a new proposal.
        let proposal = match self.proposer_agent.create_proposal() {
            Some(p) => p,
            None => {
                logger::log_warn(
                    "[Coordinator] Proposal retrieval failed (not leader or no pending requests).",
                );
                return false;
            }
        };
        let ballot = proposal.ballot;
        let slot = proposal.slot;

        // Run prepare phase.
        let prepare_result = self.prepare_phase(ballot, slot);
        let final_value = match prepare_result {
            PrepareResult::Outcome(outcome) => outcome.chosen_value,
            PrepareResult::QuorumFailure => {
                logger::log_error("[Coordinator] Prepare phase quorum failure.");
                return false;
            }
            PrepareResult::MissingProposal => {
                logger::log_error(&format!(
                    "[Coordinator] Prepare phase failed: missing proposal for slot {}.",
                    slot
                ));
                return false;
            }
        };

        // Run accept phase.
        let accept_result = self.accept_phase(final_value.clone(), slot, ballot);
        match accept_result {
            AcceptResult::Accepted(_) => {
                let learn_result = self.commit_phase(slot);
                if let Some(lr) = learn_result {
                    self.proposer_agent.finalize_proposal(slot, &final_value);
                    // TODO: Do this check better
                    if lr.learned_value.command.is_none() {
                        logger::log_error(
                            "[Coordinator] Commit phase failed: learned value is empty.",
                        );
                        return false;
                    }
                    logger::log_info(&format!(
                        "[Coordinator] Paxos round complete for slot {} with value '{:?}'.",
                        slot, lr.learned_value
                    ));
                    true
                } else {
                    false
                }
            }
            AcceptResult::QuorumFailure | AcceptResult::MissingProposal => {
                logger::log_error(
                    "[Coordinator] Accept phase failed to reach quorum or was rejected.",
                );
                false
            }
        }
    }

    /// Executes the prepare phase by merging local and remote promise responses.
    /// The coordinator queries its local acceptor for a promise and passes it along.
    fn prepare_phase(&self, ballot: Ballot, slot: Slot) -> PrepareResult {
        // Query local acceptor.
        let local_promise_result = self.acceptor_agent.process_prepare(slot, ballot);

        let local_promise = match local_promise_result {
            bindings::paxos::default::acceptor_types::PromiseResult::Promised(promise) => promise,
            bindings::paxos::default::acceptor_types::PromiseResult::Rejected(current_ballot) => {
                logger::log_error(&format!(
                    "[Coordinator] Local prepare failed for slot {} with ballot {} (current promise: {}).",
                    slot, ballot, current_ballot
                ));
                panic!("Local prepare failure") // TODO: Handle this?
            }
        };

        // Merge the local promise with remote responses using the proposer agent.
        let prepare_result = self
            .proposer_agent
            .prepare_phase(slot, ballot, &vec![local_promise]);

        logger::log_info(&format!(
            "[Coordinator] Completed prepare phase for slot {}.",
            slot
        ));
        prepare_result
    }

    /// Executes the accept phase by merging local acceptance with remote responses.
    /// The coordinator queries its local acceptor for an acceptance vote.
    fn accept_phase(&self, value: Value, slot: Slot, ballot: Ballot) -> AcceptResult {
        let local_accept_result = self
            .acceptor_agent
            .process_accept(slot, ballot, &value.clone());

        match local_accept_result {
            bindings::paxos::default::acceptor_types::AcceptedResult::Accepted(local_accept) => {
                let accept_result =
                    self.proposer_agent
                        .accept_phase(&value, slot, ballot, &vec![local_accept]);

                logger::log_info(&format!(
                    "[Coordinator] Completed accept phase for slot {}.",
                    slot
                ));
                accept_result
            }
            bindings::paxos::default::acceptor_types::AcceptedResult::Rejected(current_ballot) => {
                logger::log_error(&format!(
                    "[Coordinator] Local acceptance failed for slot {} with ballot {} (current promise: {}).",
                    slot, ballot, current_ballot
                ));
                panic!("Local acceptance failure") // TODO: Handle this?
            }
        }
    }

    /// Phase 3: Commit Phase.
    ///
    /// Commits the proposal by updating the learner and key‑value store, then broadcasting a commit message.
    fn commit_phase(&self, slot: Slot) -> Option<LearnResult> {
        if let Some(prop) = self.proposer_agent.get_proposal(slot) {
            self.learner.learn(prop.slot, &prop.client_request.value);
            self.kv_store
                .set(&self.paxos_key, &prop.client_request.value);
            let learn = Learn {
                slot,
                // ballot: prop.ballot, // TODO: Ballot needed?
                value: prop.client_request.value.clone(),
            };
            let msg = NetworkMessage {
                sender: self.node.clone(),
                payload: MessagePayload::Learn(learn),
            };
            let _ = network::send_message(&self.nodes, &msg);
            Some(LearnResult {
                learned_value: prop.client_request.value,
                quorum: self.get_quorum(),
            })
        } else {
            logger::log_warn(&format!(
                "[Coordinator] No proposal found during commit phase for slot {}.",
                slot
            ));
            None
        }
    }

    /// Retrieves the current learned value from the learner.
    fn get_learned_value(&self) -> Value {
        let state = self.learner.get_state();
        state
            .learned
            .first()
            .map(|entry| entry.value.clone())
            .expect("No learned value found!")
    }

    /// Handles the message from a remote coordinator.
    /// This function pattern-matches on the incoming NetworkMessage and calls the
    /// appropriate Paxos logic (e.g. on the acceptor, learner, or KV store),
    /// then returns a corresponding NetworkResponse.
    fn handle_message(&self, message: NetworkMessage) -> NetworkMessage {
        logger::log_info(&format!(
            "[Coordinator] Received network message: {:?}",
            message
        ));
        //* Moved the Promise and Accepted handle to the proposer agent. As will also be done by all the other handles to their respective agent. */
        match message.payload {
            MessagePayload::Learn(payload) => {
                logger::log_info(&format!(
                    "Handling LEARN: slot={}, value={:?}",
                    payload.slot, payload.value
                ));
                // Update learner and persist value in the KV store.
                self.learner.learn(payload.slot, &payload.value);
                self.kv_store.set(&self.paxos_key, &payload.value);
                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Learn(payload), // TODO: Use custom learn-ack type? Needed?
                }
            }

            //* Forward the relevant messages to the respective agents */
            MessagePayload::Prepare(_) => self.acceptor_agent.handle_message(&message),
            MessagePayload::Accept(_) => self.acceptor_agent.handle_message(&message),

            MessagePayload::Heartbeat(payload) => {
                logger::log_info(&format!(
                    "[Coordinator] Handling HEARTBEAT: sender: {:?}, timestamp={}",
                    payload.sender, payload.timestamp
                ));
                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Heartbeat(payload),
                }
            }
            other_message => {
                logger::log_warn(&format!(
                    "[Coordinator] Received irrelevant message type: {:?}",
                    other_message
                ));
                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore,
                }
            }
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

    fn elect_leader(&self) -> ElectionResult {
        // Leader election logic can be added here.
        todo!()
    }
}
