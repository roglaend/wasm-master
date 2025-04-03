use core::panic;
use std::cell::Cell;
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
    LearnResult, PaxosState, PrepareResult, RunConfig,
};
use bindings::paxos::default::network_types::{MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{ClientRequest, Learn, Node, PaxosPhase, Slot, Value};
use bindings::paxos::default::{
    acceptor_agent, failure_detector, kv_store, learner, logger, network, proposer_agent,
};

pub struct MyPaxosCoordinator;

impl CoordinatorGuest for MyPaxosCoordinator {
    type PaxosCoordinatorResource = MyPaxosCoordinatorResource;
}

pub struct MyPaxosCoordinatorResource {
    config: RunConfig,

    node: Node,
    // The list of nodes in the cluster.
    nodes: Vec<Node>,

    // Uses the agents
    proposer_agent: Arc<proposer_agent::ProposerAgentResource>,
    acceptor_agent: Arc<acceptor_agent::AcceptorAgentResource>,

    learner: Arc<learner::LearnerResource>,
    kv_store: Arc<kv_store::KvStoreResource>,
    failure_detector: Arc<failure_detector::FailureDetectorResource>,
    // A key for storing committed Paxos values in the key/value store.
    paxos_key: String,
    // heartbeats: Arc<Mutex<HashMap<u64, u64>>>,
    // node_id: u64,
    // leader_id: Cell<u64>,
    // election_in_progress: Cell<bool>
    next_commit_slot: Cell<Slot>, // Adu
}

impl MyPaxosCoordinatorResource {
    /// Returns the required quorum (here, a majority).
    fn get_quorum(&self) -> u64 {
        (self.nodes.len() as u64 / 2) + 1
    }
}

impl GuestPaxosCoordinatorResource for MyPaxosCoordinatorResource {
    /// Creates a new coordinator resource.
    fn new(node: Node, nodes: Vec<Node>, is_leader: bool, config: RunConfig) -> Self {
        let proposer_agent = Arc::new(proposer_agent::ProposerAgentResource::new(
            &node, &nodes, is_leader, config,
        ));
        let acceptor_agent = Arc::new(acceptor_agent::AcceptorAgentResource::new(
            &node, &nodes, config,
        ));
        let learner = Arc::new(learner::LearnerResource::new());
        let kv_store = Arc::new(kv_store::KvStoreResource::new());

        let failure_delta = 10; // TODO: Make this dynamic
        let failure_detector = Arc::new(failure_detector::FailureDetectorResource::new(
            node.node_id.clone(),
            &nodes,
            failure_delta,
        ));

        Self {
            config,
            node,
            nodes,
            proposer_agent,
            acceptor_agent,
            learner,
            kv_store,
            failure_detector,
            paxos_key: "paxos_values".to_string(), // TODO: Change the kv-store to save paxos values for slots instead of using the "history"
            next_commit_slot: Cell::new(1),
        }
    }

    fn submit_client_request(&self, req: ClientRequest) -> bool {
        self.proposer_agent.submit_client_request(&req)
    }

    // Ticker called from host
    fn run_paxos_loop(&self) {
        match self.proposer_agent.get_paxos_phase() {
            PaxosPhase::Start => {
                logger::log_debug("[Coordinator] Run loop: started.");
                if self.proposer_agent.is_leader() {
                    self.proposer_agent.start_leader_loop();
                }
            }
            PaxosPhase::PrepareSend => {
                logger::log_info("[Coordinator] Run loop: Running in phase one.");
                self.prepare_phase();
            }
            PaxosPhase::PreparePending => {
                logger::log_info("[Coordinator] Run loop: Checking Prepare timeout.");
                self.proposer_agent.check_prepare_timeout();
            }
            PaxosPhase::AcceptCommit => {
                logger::log_info("[Coordinator] Run loop: Running in phase two.");
                self.accept_phase();
                if !self.config.acceptors_send_learns {
                    self.commit_phase();
                }
            }
            PaxosPhase::Stop => {}  // TODO
            PaxosPhase::Crash => {} // TODO
        }
    }

    /// Executes the prepare phase by merging local and remote promise responses.
    /// The coordinator queries its local acceptor for a promise and passes it along.
    fn prepare_phase(&self) -> PrepareResult {
        let slot = self.proposer_agent.get_current_slot();
        let ballot = self.proposer_agent.get_current_ballot();

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
    fn accept_phase(&self) -> AcceptResult {
        let proposal = match self.proposer_agent.create_proposal() {
            Some(p) => p,
            None => {
                logger::log_debug("[Coordinator] Accept phase aborted: no proposal available.");
                return AcceptResult::MissingProposal;
            }
        };

        let local_accept_result =
            self.acceptor_agent
                .process_accept(proposal.slot, proposal.ballot, &proposal.value);

        match local_accept_result {
            bindings::paxos::default::acceptor_types::AcceptedResult::Accepted(local_accept) => {
                let accept_result = self.proposer_agent.accept_phase(
                    &proposal.value,
                    proposal.slot,
                    proposal.ballot,
                    &vec![local_accept],
                );

                logger::log_info(&format!(
                    "[Coordinator] Completed accept phase for slot {}.",
                    proposal.slot
                ));
                accept_result
            }
            bindings::paxos::default::acceptor_types::AcceptedResult::Rejected(current_ballot) => {
                logger::log_error(&format!(
                    "[Coordinator] Local acceptance failed for slot {} with ballot {} (current promise: {}).",
                    proposal.slot, proposal.ballot, current_ballot
                ));
                panic!("Local acceptance failure") // TODO: Handle this?
            }
        }
    }

    /// Commits the proposal by updating the learner and key‑value store, then broadcasting a commit message.
    fn commit_phase(&self) -> Option<LearnResult> {
        let next_slot = self.next_commit_slot.get();

        if let Some(prop) = self.proposer_agent.get_accepted_proposal(next_slot) {
            self.learner.learn(prop.slot, &prop.value); // TODO: Properly handle the order/execution of learns inside the learners
            self.kv_store.set(&self.paxos_key, &prop.value); // TODO: Move this to inside the learner
            let learn = Learn {
                slot: prop.slot,
                // ballot: prop.ballot, // TODO: Ballot needed?
                value: prop.value.clone(),
            };
            let msg = NetworkMessage {
                sender: self.node.clone(),
                payload: MessagePayload::Learn(learn),
            };
            if self.config.is_event_driven {
                network::send_message_forget(&self.nodes, &msg);
            } else {
                let _ = network::send_message(&self.nodes, &msg);
                // TODO: Handle the case if we send learn-ack back?
            }

            // Update the next commit slot for the next round.
            self.next_commit_slot.set(next_slot + 1);

            Some(LearnResult {
                learned_value: prop.value,
                quorum: self.get_quorum(),
            })
        } else {
            logger::log_debug(&format!(
                "[Coordinator] No accepted proposal found during commit phase for slot {}.",
                next_slot
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
    fn handle_message(&self, message: NetworkMessage) -> NetworkMessage {
        logger::log_debug(&format!(
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
                if !self.config.acceptors_send_learns {
                    // Update learner and persist value in the KV store. // TODO: Properly handle the order/execution of learns inside the learners
                    self.learner.learn(payload.slot, &payload.value);
                    self.kv_store.set(&self.paxos_key, &payload.value); // TODO: Move this to inside the learner
                }
                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Learn(payload), // TODO: Use custom learn-ack type? Needed?
                }
            }

            //* Forward the relevant messages to the respective agents */
            MessagePayload::Promise(_) => self.proposer_agent.handle_message(&message),
            MessagePayload::Accepted(_) => self.proposer_agent.handle_message(&message),

            MessagePayload::Prepare(_) => self.acceptor_agent.handle_message(&message),
            MessagePayload::Accept(_) => self.acceptor_agent.handle_message(&message),

            MessagePayload::Heartbeat(payload) => {
                logger::log_debug(&format!(
                    "[Coordinator] Handling HEARTBEAT: sender: {:?}, timestamp={}",
                    payload.sender, payload.timestamp
                ));
                self.failure_detector
                    .heartbeat(payload.sender.clone().node_id);

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
        todo!()
    }

    fn failure_service(&self) {
        // TODO : What to do with leader change
        // TODO : Also need to handle change in timeout
        let new_lead = self.failure_detector.checker();
        if let Some(leader) = new_lead {
            logger::log_warn(&format!("Leader {} change initiated. New leader", &leader));
            if leader == self.node.node_id {
                self.proposer_agent.become_leader();
            }
        }
    }
}
