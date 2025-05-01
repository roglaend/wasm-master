use core::panic;
use std::sync::Arc;

pub mod bindings {
    wit_bindgen::generate!( {
        path: "../../shared/wit",
        world: "paxos-coordinator-world",
        additional_derives: [Clone, Hash],
    });
}

bindings::export!(MyCoordinator with_types_in bindings);

use bindings::exports::paxos::default::paxos_coordinator::{
    AcceptResult, ElectionResult, Guest, GuestPaxosCoordinatorResource, PaxosState, PrepareResult,
    RunConfig,
};
use bindings::paxos::default::acceptor_types::{AcceptedResult, PromiseResult};
use bindings::paxos::default::learner_types::RetryLearnResult;
use bindings::paxos::default::network_types::{MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{ClientResponse, Node, PaxosPhase, PaxosRole, Value};
use bindings::paxos::default::{
    acceptor_agent, failure_detector, learner_agent, logger, proposer_agent,
};

pub struct MyCoordinator;

impl Guest for MyCoordinator {
    type PaxosCoordinatorResource = MyPaxosCoordinatorResource;
}

pub struct MyPaxosCoordinatorResource {
    config: RunConfig,

    node: Node,
    nodes: Vec<Node>,

    // Uses the agents
    proposer_agent: Arc<proposer_agent::ProposerAgentResource>,
    acceptor_agent: Arc<acceptor_agent::AcceptorAgentResource>,
    learner_agent: Arc<learner_agent::LearnerAgentResource>,

    failure_detector: Arc<failure_detector::FailureDetectorResource>,
}

impl MyPaxosCoordinatorResource {
    fn maybe_retry_learn(&self) {
        match self.learner_agent.evaluate_retry() {
            RetryLearnResult::NoGap => return,
            RetryLearnResult::Skip(slot) => {
                logger::log_debug(&format!(
                    "[Coordinator] Learner skipping retry for slot {}",
                    slot
                ));
                return;
            }
            RetryLearnResult::Retry(slot) => {
                logger::log_warn(&format!(
                    "[Coordinator] Learner detected gap, retrying learn for slot {}",
                    slot
                ));
                if self.config.acceptors_send_learns {
                    // self.acceptor_agent.retry_learn(slot); // TODO
                } else {
                    self.proposer_agent.retry_learn(slot);
                }
            }
        }
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

        let learner_agent = Arc::new(learner_agent::LearnerAgentResource::new(
            &node, &nodes, config,
        ));

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
            learner_agent,
            failure_detector,
        }
    }

    fn submit_client_request(&self, req: Value) -> bool {
        self.proposer_agent.submit_client_request(&req)
    }

    fn is_leader(&self) -> bool {
        self.proposer_agent.is_leader()
    }

    // Ticker called from runner
    fn run_paxos_loop(&self) -> Option<Vec<ClientResponse>> {
        match self.node.role {
            PaxosRole::Client => {
                panic!("[Coordinator] Client nodes should never call run_paxos_loop");
            }

            PaxosRole::Proposer => self.proposer_agent.run_paxos_loop(),

            PaxosRole::Acceptor => {
                // self.acceptor_agent.run_paxos_loop(); // TODO
                None
            }

            PaxosRole::Learner => {
                self.learner_agent.run_paxos_loop();
                None
            }

            PaxosRole::Coordinator => {
                if !self.proposer_agent.is_leader() {
                    self.maybe_retry_learn();
                    return None;
                }

                match self.proposer_agent.get_paxos_phase() {
                    PaxosPhase::Start => {
                        self.proposer_agent.start_leader_loop();
                        None
                    }
                    PaxosPhase::PrepareSend => {
                        let _ = self.prepare_phase();
                        None
                    }
                    PaxosPhase::PreparePending => {
                        self.proposer_agent.check_prepare_timeout();
                        None
                    }
                    PaxosPhase::AcceptCommit => {
                        let _ = self.accept_phase();
                        self.commit_phase()
                    }
                    PaxosPhase::Stop | PaxosPhase::Crash => None,
                }
            }
        }
    }

    /// Executes the prepare phase by merging local and remote promise responses.
    /// The coordinator queries its local acceptor for a promise and passes it along.
    fn prepare_phase(&self) -> PrepareResult {
        // Starts from 1, or the number the learner has "executed" on leader change.
        let slot = self.learner_agent.get_next_to_execute() - 1;
        let ballot = self.proposer_agent.get_current_ballot();

        // Query local acceptor.
        let local_promise_result = self.acceptor_agent.process_prepare(slot, ballot);

        let local_promise = match local_promise_result {
            PromiseResult::Promised(promise) => promise,
            PromiseResult::Rejected(current_ballot) => {
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

    fn accept_phase(&self) -> AcceptResult {
        let props = self.proposer_agent.proposals_to_accept();

        if props.is_empty() {
            logger::log_debug("[Coordinator] Accept phase aborted: no proposal available.");
            return AcceptResult::MissingProposal;
        }

        // For each proposal, do a local accept first then make proposer send accept messages to rest.
        for p in props {
            let local_acc = self
                .acceptor_agent
                .process_accept(p.slot, p.ballot, &p.value);

            match local_acc {
                AcceptedResult::Accepted(acc) => {
                    let accept_result =
                        self.proposer_agent
                            .accept_phase(&p.value, p.slot, p.ballot, &vec![acc]);

                    logger::log_info(&format!(
                        "[Coordinator] Completed accept phase for slot {}.",
                        p.slot
                    ));

                    if let AcceptResult::IsEventDriven = accept_result {
                    } else {
                        logger::log_error("[Coordinator] Only event-driven accept is supported!");
                        panic!("Coordinator.accept_phase only supports event-driven");
                    }
                }

                AcceptedResult::Rejected(curr) => {
                    logger::log_error(&format!(
                        "[Coordinator] Local acceptance failed for slot {} with ballot {} (promise: {})",
                        p.slot, p.ballot, curr
                    ));
                    panic!("Local acceptance failure in coordinator.accept_phase");
                }
            }
        }

        // TODO: Make a better result, as this is now always true
        AcceptResult::IsEventDriven
    }

    fn commit_phase(&self) -> Option<Vec<ClientResponse>> {
        let to_commit = self.proposer_agent.learns_to_commit();
        if to_commit.is_empty() {
            return None;
        }

        for learn in to_commit {
            self.proposer_agent.broadcast_learn(&learn);

            let executed = self
                .learner_agent
                .learn_and_execute(learn.slot, &learn.value);
            _ = self.proposer_agent.process_executed(&executed);
        }

        let mut client_responses = Vec::new();
        for resp in self.proposer_agent.collect_client_responses() {
            logger::log_info(&format!(
                "[Coordinator] Client response to send back: {:?}",
                resp
            ));
            client_responses.push(resp);
        }

        if client_responses.is_empty() {
            None
        } else {
            Some(client_responses)
        }
    }

    /// Handles the message from a remote coordinator.
    fn handle_message(&self, message: NetworkMessage) -> NetworkMessage {
        logger::log_debug(&format!(
            "[Coordinator] Received network message: {:?}",
            message
        ));
        let ignore_msg = NetworkMessage {
            sender: self.node.clone(),
            payload: MessagePayload::Ignore,
        };
        match message.payload {
            //* Forward the relevant messages to the respective agents */
            MessagePayload::Promise(_) => self.proposer_agent.handle_message(&message),
            MessagePayload::Accepted(_) => self.proposer_agent.handle_message(&message),
            MessagePayload::RetryLearn(_) => self.proposer_agent.handle_message(&message),

            MessagePayload::Executed(_) => {
                //* Not needed by non-leader coordinators due to them having access to "adu" by the learners "next_to_execute" */
                // self.proposer_agent.handle_message(&message)
                ignore_msg
            }

            MessagePayload::Prepare(_) => self.acceptor_agent.handle_message(&message),
            MessagePayload::Accept(_) => self.acceptor_agent.handle_message(&message),

            MessagePayload::Learn(_) => self.learner_agent.handle_message(&message),

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
                ignore_msg
            }
        }
    }

    /// Expose a snapshot of the current Paxos state.
    /// This aggregates the learner state (learned entries) and the key/value store state.
    fn get_state(&self) -> PaxosState {
        let (learner_state, kv_state) = self.learner_agent.get_state();
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
