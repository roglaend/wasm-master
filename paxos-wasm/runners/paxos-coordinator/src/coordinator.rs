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

enum Agents {
    Proposer(Arc<proposer_agent::ProposerAgentResource>),
    Acceptor(Arc<acceptor_agent::AcceptorAgentResource>),
    Learner(Arc<learner_agent::LearnerAgentResource>),
    Coordinator {
        proposer: Arc<proposer_agent::ProposerAgentResource>,
        acceptor: Arc<acceptor_agent::AcceptorAgentResource>,
        learner: Arc<learner_agent::LearnerAgentResource>,
    },
}

pub struct MyPaxosCoordinatorResource {
    config: RunConfig,

    node: Node,
    nodes: Vec<Node>,

    agents: Agents,

    failure_detector: Arc<failure_detector::FailureDetectorResource>,
}

impl MyPaxosCoordinatorResource {
    fn proposer(&self) -> &proposer_agent::ProposerAgentResource {
        match &self.agents {
            Agents::Proposer(p) => &*p,
            Agents::Coordinator { proposer, .. } => &*proposer,
            _ => panic!("Tried to use proposer on a non-proposer node"),
        }
    }

    fn acceptor(&self) -> &acceptor_agent::AcceptorAgentResource {
        match &self.agents {
            Agents::Acceptor(a) => &*a,
            Agents::Coordinator { acceptor, .. } => &*acceptor,
            _ => panic!("Tried to use acceptor on a non-acceptor node"),
        }
    }

    fn learner(&self) -> &learner_agent::LearnerAgentResource {
        match &self.agents {
            Agents::Learner(l) => &*l,
            Agents::Coordinator { learner, .. } => &*learner,
            _ => panic!("Tried to use learner on a non-learner node"),
        }
    }

    fn maybe_retry_learn(&self) {
        match self.learner().evaluate_retry() {
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
                    self.acceptor().retry_learn(slot); // TODO
                } else {
                    self.proposer().retry_learn(slot);
                }
            }
        }
    }

    fn drive_learner(&self) {
        self.learner().execute_chosen_learns();

        // TODO: Might add a max batch or require a full batch, but don't think that's valid here.
        let exec = self.learner().collect_executed(None, false);

        if self.proposer().is_leader() && !exec.results.is_empty() {
            let _ = self.proposer().process_executed(&exec);
        }
    }
}

impl GuestPaxosCoordinatorResource for MyPaxosCoordinatorResource {
    /// Creates a new coordinator resource.
    fn new(node: Node, nodes: Vec<Node>, is_leader: bool, config: RunConfig) -> Self {
        let agents = match node.role {
            PaxosRole::Proposer => Agents::Proposer(Arc::new(
                proposer_agent::ProposerAgentResource::new(&node, &nodes, is_leader, config),
            )),
            PaxosRole::Acceptor => Agents::Acceptor(Arc::new(
                acceptor_agent::AcceptorAgentResource::new(&node, &nodes, config),
            )),
            PaxosRole::Learner => Agents::Learner(Arc::new(
                learner_agent::LearnerAgentResource::new(&node, &nodes, config),
            )),
            PaxosRole::Coordinator => {
                let proposer = Arc::new(proposer_agent::ProposerAgentResource::new(
                    &node, &nodes, is_leader, config,
                ));
                let acceptor = Arc::new(acceptor_agent::AcceptorAgentResource::new(
                    &node, &nodes, config,
                ));
                let learner = Arc::new(learner_agent::LearnerAgentResource::new(
                    &node, &nodes, config,
                ));
                Agents::Coordinator {
                    proposer,
                    acceptor,
                    learner,
                }
            }
            PaxosRole::Client => unreachable!(),
        };

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
            agents,
            failure_detector,
        }
    }

    fn submit_client_request(&self, req: Value) -> bool {
        match &self.agents {
            Agents::Proposer(p) | Agents::Coordinator { proposer: p, .. } => {
                p.submit_client_request(&req)
            }
            _ => false,
        }
    }

    fn is_leader(&self) -> bool {
        match &self.agents {
            Agents::Proposer(p) | Agents::Coordinator { proposer: p, .. } => p.is_leader(),
            _ => false,
        }
    }

    // Ticker called from runner
    fn run_paxos_loop(&self) -> Option<Vec<ClientResponse>> {
        match &self.agents {
            Agents::Proposer(proposer) => {
                return proposer.run_paxos_loop();
            }

            Agents::Acceptor(_acceptor) => {
                // acceptor.run_paxos_loop(); // TODO: ?
                return None;
            }

            Agents::Learner(lea) => {
                lea.run_paxos_loop();
                return None;
            }

            Agents::Coordinator {
                proposer,
                acceptor: _,
                learner: _,
            } => {
                self.maybe_retry_learn();
                self.drive_learner();

                if !proposer.is_leader() {
                    return None;
                }

                match proposer.get_paxos_phase() {
                    PaxosPhase::Start => {
                        proposer.start_leader_loop();
                        None
                    }
                    PaxosPhase::PrepareSend => {
                        let _ = self.prepare_phase();
                        None
                    }
                    PaxosPhase::PreparePending => {
                        proposer.check_prepare_timeout();
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
        // Starts from 1, or the adu on the learner, the highest slot the learner has chosen.
        let slot = self.learner().get_adu() + 1;
        let ballot = self.proposer().get_current_ballot();

        // Query local acceptor.
        let local_promise_result = self.acceptor().process_prepare(slot, ballot);

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
            .proposer()
            .prepare_phase(slot, ballot, &vec![local_promise]);

        logger::log_info(&format!(
            "[Coordinator] Completed prepare phase for slot {}.",
            slot
        ));
        prepare_result
    }

    fn accept_phase(&self) -> AcceptResult {
        let props = self.proposer().proposals_to_accept();

        if props.is_empty() {
            logger::log_debug("[Coordinator] Accept phase aborted: no proposal available.");
            return AcceptResult::MissingProposal;
        }

        // For each proposal, do a local accept first then make proposer send accept messages to rest.
        for p in props {
            let local_acc = self.acceptor().process_accept(p.slot, p.ballot, &p.value);

            match local_acc {
                AcceptedResult::Accepted(acc) => {
                    // Sends Accept to all acceptors
                    let accept_result =
                        self.proposer()
                            .accept_phase(&p.value, p.slot, p.ballot, &vec![acc]);

                    if self.config.acceptors_send_learns {
                        if let Some(learn) =
                            // Sends Learn to all learners
                            self.acceptor().commit_phase(p.slot, &p.value.clone())
                        {
                            let _ =
                                self.learner()
                                    .record_learn(learn.slot, &learn.value, &self.node);
                        }
                    }

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
        if !self.config.acceptors_send_learns {
            let to_commit = self.proposer().learns_to_commit();
            if !to_commit.is_empty() {
                for learn in to_commit {
                    self.proposer().broadcast_learn(&learn);
                    self.learner()
                        .record_learn(learn.slot, &learn.value, &self.node);
                }
            }
        }

        let responses = self.proposer().collect_client_responses();
        (!responses.is_empty()).then(|| {
            logger::log_info(&format!(
                "[Coordinator] Sending {} client responses",
                responses.len()
            ));
            responses
        })
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
        match &self.agents {
            //* Forward the relevant messages to the respective agents */
            Agents::Proposer(proposer) => match message.payload {
                MessagePayload::Promise(_) => proposer.handle_message(&message),
                MessagePayload::Accepted(_) => proposer.handle_message(&message),
                MessagePayload::RetryLearn(_) => {
                    if !self.config.acceptors_send_learns {
                        proposer.handle_message(&message)
                    } else {
                        ignore_msg
                    }
                }
                MessagePayload::Executed(_) => proposer.handle_message(&message),
                _ => {
                    logger::log_warn(&format!(
                        "[Coordinator] Received irrelevant message type for Proposer: {:?}",
                        message.payload
                    ));
                    ignore_msg
                }
            },

            Agents::Acceptor(acceptor) => match message.payload {
                MessagePayload::Prepare(_) => acceptor.handle_message(&message),
                MessagePayload::Accept(_) => acceptor.handle_message(&message),
                MessagePayload::RetryLearn(_) => {
                    if self.config.acceptors_send_learns {
                        acceptor.handle_message(&message)
                    } else {
                        ignore_msg
                    }
                }
                _ => {
                    logger::log_warn(&format!(
                        "[Coordinator] Received irrelevant message type for Acceptor: {:?}",
                        message.payload
                    ));
                    ignore_msg
                }
            },

            Agents::Learner(learner) => match message.payload {
                MessagePayload::Learn(_) => learner.handle_message(&message),
                _ => {
                    logger::log_warn(&format!(
                        "[Coordinator] Received irrelevant message type for Learner: {:?}",
                        message.payload
                    ));
                    ignore_msg
                }
            },

            Agents::Coordinator {
                proposer,
                acceptor,
                learner,
            } => match message.payload {
                MessagePayload::Promise(_) => proposer.handle_message(&message),
                MessagePayload::Accepted(_) => proposer.handle_message(&message),

                MessagePayload::RetryLearn(_) => {
                    if !self.config.acceptors_send_learns {
                        proposer.handle_message(&message)
                    } else {
                        acceptor.handle_message(&message)
                    }
                }

                MessagePayload::Executed(_) => {
                    if self.node.role != PaxosRole::Coordinator {
                        proposer.handle_message(&message);
                    }
                    //* Not needed by non-leader coordinators due to them having access to "adu" through their learners */
                    ignore_msg
                }

                MessagePayload::Prepare(_) => acceptor.handle_message(&message),
                MessagePayload::Accept(_) => acceptor.handle_message(&message),

                MessagePayload::Learn(_) => learner.handle_message(&message),

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
                _ => {
                    logger::log_warn(&format!(
                        "[Coordinator] Received irrelevant message type: {:?}",
                        message.payload
                    ));
                    ignore_msg
                }
            },
        }
    }

    /// Expose a snapshot of the current Paxos state.
    /// This aggregates the learner state (learned entries) and the key/value store state.
    fn get_state(&self) -> PaxosState {
        let (learner_state, kv_state) = self.learner().get_state();
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
                self.proposer().become_leader();
            }
        }
    }
}
