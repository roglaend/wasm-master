use rand::Rng;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "proposer-agent-world",
        // generate_unused_types: true,
        additional_derives: [PartialEq, Clone],
    });
}

bindings::export!(MyProposerAgent with_types_in bindings);

use bindings::exports::paxos::default::proposer_agent::{
    Guest as GuestProposerAgent, GuestProposerAgentResource,
};
use bindings::paxos::default::network_types::{Heartbeat, MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{
    Accept, Accepted, Ballot, ClientResponse, Executed, Learn, Node, PaxosPhase, PaxosRole,
    Prepare, Promise, Proposal, RunConfig, Slot, Value,
};
use bindings::paxos::default::proposer_types::{AcceptResult, PrepareResult, ProposalStatus};
use bindings::paxos::default::{
    failure_detector::FailureDetectorResource, logger, network, proposer::ProposerResource,
};

enum CollectedResponses<T, R> {
    Synchronous(Vec<T>),
    EventDriven(R),
}

pub struct MyProposerAgent;

impl GuestProposerAgent for MyProposerAgent {
    type ProposerAgentResource = MyProposerAgentResource;
}

pub struct MyProposerAgentResource {
    config: RunConfig,
    phase: Cell<PaxosPhase>,

    node: Node,
    proposer: Arc<ProposerResource>,
    num_acceptors: usize,

    acceptors: Vec<Node>,
    learners: Vec<Node>,
    failure_detector: Arc<FailureDetectorResource>,

    // A mapping from Ballot to unique promises per node_id.
    promises: RefCell<BTreeMap<Ballot, HashMap<u64, Promise>>>,
    in_flight_accepted: RefCell<BTreeMap<Slot, HashMap<u64, Accepted>>>, //* Per slot accepted per sender */

    client_responses: RefCell<BTreeMap<Slot, ClientResponse>>,

    adu: Cell<u64>,

    last_prepare_start: Cell<Option<Instant>>, // Need a timeout mechanism to retry the prepare phase in case of failure (?)

    batch_size: u64,

    test_skip_slot: Cell<u64>,
}

impl MyProposerAgentResource {
    fn get_quorum_acceptors(&self) -> usize {
        (self.num_acceptors / 2) + 1
    }

    /// Sets the new phase with logging.
    fn set_phase(&self, new_phase: PaxosPhase) {
        let old_phase = self.phase.get();
        if old_phase != new_phase {
            logger::log_debug(&format!(
                "[Proposer Agent] Transitioning from phase {:?} to {:?}",
                old_phase, new_phase
            ));
            self.phase.set(new_phase);
        }
    }

    /// Advances the phase based on the outcome of the prepare round.
    fn advance_phase(&self, prepare_success: Option<bool>) {
        let new_phase = match self.phase.get() {
            PaxosPhase::Start => PaxosPhase::PrepareSend,
            PaxosPhase::PrepareSend => PaxosPhase::PreparePending,
            PaxosPhase::PreparePending => match prepare_success {
                Some(true) => PaxosPhase::AcceptCommit,
                Some(false) | None => PaxosPhase::PrepareSend,
            },
            phase => phase, // For AcceptCommit, Stop, or Crash.
        };
        self.set_phase(new_phase);
    }

    /// Called when starting a new prepare round.
    fn start_prepare_round(&self) {
        self.last_prepare_start.set(Some(Instant::now()));
        self.advance_phase(None); // moves from PrepareSend to PreparePending
    }

    fn insert_and_check_promises(
        &self,
        promise: Promise,
        sender: &Node,
    ) -> Option<(Ballot, Vec<Promise>)> {
        // Insert the promise into our nested map.
        self.promises
            .borrow_mut()
            .entry(promise.ballot)
            .or_insert_with(HashMap::new)
            .insert(sender.node_id, promise.clone());

        // Check for quorum in the highest ballot group.
        let promises = self.promises.borrow();
        if let Some((&highest_ballot, promise_map)) = promises.iter().next_back() {
            let quorum = self.get_quorum_acceptors();
            let count = promise_map.len();
            logger::log_info(&format!(
                "[Proposer Agent] Highest ballot {} has {} unique promises (quorum: {}).",
                highest_ballot, count, quorum
            ));
            // Might be a a quorum of valid promises
            if count >= quorum {
                let promise_list: Vec<Promise> = promise_map.values().cloned().collect();
                return Some((highest_ballot, promise_list));
            }
        }
        None
    }

    fn insert_and_check_accepted(
        &self,
        accepted: Accepted,
        sender: &Node,
    ) -> Option<(Slot, Vec<Accepted>)> {
        let slot = accepted.slot;
        self.in_flight_accepted
            .borrow_mut()
            .entry(slot)
            .or_insert_with(HashMap::new)
            .insert(sender.node_id, accepted.clone());

        let accepted_map = self.in_flight_accepted.borrow();
        if let Some(sender_map) = accepted_map.get(&slot) {
            let quorum = self.get_quorum_acceptors();
            let count = sender_map.len();
            logger::log_info(&format!(
                "[Proposer Agent] For slot {} received {} accepted responses (quorum: {}).",
                slot, count, quorum
            ));
            // Might be a a quorum of valid accepted
            if count >= quorum {
                let accepted_list: Vec<Accepted> = sender_map.values().cloned().collect();
                return Some((slot, accepted_list));
            }
        }
        None
    }

    fn process_promises(
        &self,
        slot: Slot,
        ballot: Ballot,
        promises: Vec<Promise>,
    ) -> PrepareResult {
        logger::log_debug(&format!(
            "[Proposer Agent] Processing {} promises for ballot {}.",
            promises.len(),
            ballot
        ));
        self.proposer.process_prepare(slot, &promises)
    }

    fn process_accepted(&self, slot: Slot, accepts: Vec<Accepted>) -> AcceptResult {
        logger::log_debug(&format!(
            "[Proposer Agent] Processing {} accepted responses for slot {}.",
            accepts.len(),
            slot
        ));
        self.proposer.process_accept(slot, &accepts)
    }

    /// Helper function to send a prepare message and collect responses if not event driven.
    fn broadcast_prepare(
        &self,
        slot: Slot,
        ballot: u64,
    ) -> CollectedResponses<Promise, PrepareResult> {
        let prepare = Prepare { slot, ballot };
        let msg = NetworkMessage {
            sender: self.node.clone(),
            payload: MessagePayload::Prepare(prepare),
        };

        logger::log_debug(&format!(
            "[Proposer Agent] Broadcasting PREPARE: slot={}, ballot={}.",
            slot, ballot
        ));

        if !self.config.is_event_driven {
            // In synchronous mode, send and collect responses.
            let responses = network::send_message(&self.acceptors, &msg);
            let mut promises = vec![];
            for resp in responses {
                if let MessagePayload::Promise(prom_payload) = resp.payload {
                    promises.push(prom_payload);
                } else {
                    logger::log_warn(
                        "[Proposer Agent] Unexpected payload variant in prepare phase.",
                    );
                }
            }
            CollectedResponses::Synchronous(promises)
        } else {
            logger::log_debug(
                "[Proposer Agent] Event-driven mode enabled. Sending fire-and-forget prepare messages.",
            );
            network::send_message_forget(&self.acceptors, &msg);
            CollectedResponses::EventDriven(PrepareResult::IsEventDriven)
        }
    }

    /// Helper function to send an accept message and collect responses if not event driven.
    fn broadcast_accept(
        &self,
        value: Value,
        slot: Slot,
        ballot: Ballot,
    ) -> CollectedResponses<Accepted, AcceptResult> {
        let accept = Accept {
            slot,
            ballot,
            value: value.clone(),
        };
        let msg = NetworkMessage {
            sender: self.node.clone(),
            payload: MessagePayload::Accept(accept),
        };

        logger::log_info(&format!(
            "[Proposer Agent] Broadcasting ACCEPT: slot={}, ballot={}, value={:?}.",
            slot, ballot, value
        ));

        if !self.config.is_event_driven {
            // In synchronous mode, send and collect responses.
            let responses = network::send_message(&self.acceptors, &msg);
            let mut accepted = vec![];
            for resp in responses {
                if let MessagePayload::Accepted(acc_payload) = resp.payload {
                    accepted.push(acc_payload);
                } else {
                    logger::log_warn(
                        "[Proposer Agent] Unexpected payload variant in accept phase.",
                    );
                }
            }
            CollectedResponses::Synchronous(accepted)
        } else {
            logger::log_debug(
                "[Proposer Agent] Event-driven mode enabled. Sending fire-and-forget accept messages.",
            );
            network::send_message_forget(&self.acceptors, &msg);
            CollectedResponses::EventDriven(AcceptResult::IsEventDriven)
        }
    }

    /// Handles the result of processing prepare responses.
    fn handle_prepare_result(&self, ballot: Ballot, prepare_result: PrepareResult) {
        match prepare_result {
            PrepareResult::Success => {
                logger::log_info("[Proposer Agent] Phase one finished. Progressing to phase two.");
                let prepare_success = Some(true);
                self.advance_phase(prepare_success); // TODO: Place this in a less "hidden" place?
            }
            PrepareResult::QuorumFailure => {
                let total = self.acceptors.len();
                let count = self
                    .promises
                    .borrow()
                    .get(&ballot)
                    .map(|m| m.len())
                    .unwrap_or(0);
                if count < total {
                    logger::log_debug(&format!(
                        "[Proposer Agent] Prepare quorum failure: {} of {} responses.",
                        count, total
                    ));
                } else {
                    logger::log_error(&format!(
                        "[Proposer Agent] Prepare quorum failure: all {} responded for ballot {}, but quorum not met.",
                        total, ballot
                    ));
                }
            }
            PrepareResult::IsEventDriven => {
                logger::log_warn("[Proposer Agent] Received IsEventDriven in prepare phase.");
            }
        }
    }

    /// Handles the result of processing accept responses.
    fn handle_accept_result(&self, slot: Slot, accept_result: AcceptResult) {
        match accept_result {
            AcceptResult::Accepted(accepted_count) => {
                logger::log_info(&format!(
                    "[Proposer Agent] Accept outcome for slot {}: {} acceptances received.",
                    slot, accepted_count
                ));
                self.mark_proposal_chosen(slot);
            }
            AcceptResult::QuorumFailure => {
                let total = self.acceptors.len();
                let count = self
                    .in_flight_accepted
                    .borrow()
                    .get(&slot)
                    .map(|m| m.len())
                    .unwrap_or(0);
                if count < total {
                    logger::log_debug(&format!(
                        "[Proposer Agent] Accept quorum failure: {} of {} responses.",
                        count, total
                    ));
                } else {
                    logger::log_error(&format!(
                        "[Proposer Agent] Accept quorum failure: all {} responded for slot {}, but quorum not met.",
                        total, slot
                    ));
                }
            }
            AcceptResult::MissingProposal => {
                // logger::log_error(&format!(
                //     "[Proposer Agent] Accept phase missing in-flight proposal for slot {}.",
                //     slot
                // ));
            }
            AcceptResult::IsEventDriven => {
                logger::log_warn("[Proposer Agent] Received IsEventDriven in accept phase.");
            }
        }
    }

    // TODO: Decouple the steps inside more to be able to balance better.
    fn accept_commit(&self) -> Option<Vec<ClientResponse>> {
        for prop in self.proposals_to_accept() {
            let accept_result = self.accept_phase(
                prop.value.clone(),
                prop.slot,
                prop.ballot,
                vec![], // no initial accepts
            );
            if let AcceptResult::IsEventDriven = accept_result {
                // OK, keep going
            } else {
                logger::log_error("[Proposer Agent] Run loop is only supporting event driven.");
                panic!("accept_commit only supports event-driven accepts");
            }
        }

        if !self.config.acceptors_send_learns {
            for learn in self.learns_to_commit() {
                self.broadcast_learn(learn);
            }
        }

        let mut client_responses = Vec::new();
        for resp in self.collect_client_responses() {
            logger::log_info(&format!(
                "[Proposer Agent] Client response to send back: {:?}",
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
}

impl GuestProposerAgentResource for MyProposerAgentResource {
    fn new(node: Node, nodes: Vec<Node>, is_leader: bool, config: RunConfig) -> Self {
        let acceptors: Vec<_> = nodes
            .clone()
            .into_iter()
            .filter(|x| x.role == PaxosRole::Acceptor || x.role == PaxosRole::Coordinator)
            .collect();
        // TODO: make this more future proof?
        let num_acceptors = acceptors.len() as u64
            + if node.role == PaxosRole::Coordinator {
                1
            } else {
                0
            };

        let learners: Vec<_> = nodes
            .clone()
            .into_iter()
            .filter(|x| x.role == PaxosRole::Learner || x.role == PaxosRole::Coordinator)
            .collect();

        let num_learners = learners.len() as u64
            + if node.role == PaxosRole::Coordinator {
                1
            } else {
                0
            };

        let init_ballot = node.node_id;
        let proposer = Arc::new(ProposerResource::new(
            is_leader,
            num_acceptors,
            init_ballot,
            &node.node_id.to_string(),
        ));

        match proposer.load_state() {
            Ok(_) => logger::log_info("[Proposer Agent] Loaded state successfully."),
            Err(e) => logger::log_warn(&format!(
                "[Proposer Agent] Failed to load state. Ignore if no state saved: {}",
                e
            )),
        }

        logger::log_info(&format!(
            "[Proposer Agent] Initialized with node_id={} as {} leader. ({} acceptors, {} learners)",
            node.node_id,
            if is_leader { "a" } else { "not a" },
            num_acceptors,
            num_learners
        ));

        let failure_delta = 10; // TODO: Make this dynamic
        let failure_detector = Arc::new(FailureDetectorResource::new(
            node.node_id.clone(),
            &nodes,
            failure_delta,
        ));

        let batch_size = 20; // TODO: make part of config/input

        Self {
            config,
            phase: Cell::new(PaxosPhase::Start),

            node,
            proposer,
            acceptors,
            num_acceptors: num_acceptors as usize,
            learners,
            promises: RefCell::new(BTreeMap::new()),
            in_flight_accepted: RefCell::new(BTreeMap::new()),
            last_prepare_start: Cell::new(None),
            failure_detector,

            client_responses: RefCell::new(BTreeMap::new()),
            adu: Cell::new(0),
            batch_size,
            test_skip_slot: Cell::new(10),
        }
    }

    fn get_paxos_phase(&self) -> PaxosPhase {
        return self.phase.get();
    }

    /// Submits a client request to the core proposer.
    fn submit_client_request(&self, req: Value) -> bool {
        let result = self.proposer.enqueue_client_request(&req);
        logger::log_info(&format!(
            "[Proposer Agent] Submitted client request '{:?}': {}.",
            req, result
        ));
        // TESTING : REMOVE THIS
        // for prop in self.proposals_to_accept() {
        //     let accept_result = self.accept_phase(
        //         prop.value.clone(),
        //         prop.slot,
        //         prop.ballot,
        //         vec![], // no initial accepts
        //     );
        //     if let AcceptResult::IsEventDriven = accept_result {
        //         // OK, keep going
        //     } else {
        //         logger::log_error("[Proposer Agent] Run loop is only supporting event driven.");
        //         panic!("accept_commit only supports event-driven accepts");
        //     }
        // }
        result
    }

    fn get_current_slot(&self) -> Slot {
        self.proposer.get_current_slot()
    }

    fn get_current_ballot(&self) -> Slot {
        self.proposer.get_current_ballot()
    }

    fn get_proposal_by_status(&self, slot: Slot, ps: ProposalStatus) -> Option<Proposal> {
        self.proposer.get_proposal_by_status(slot, ps)
    }

    fn reserve_next_chosen_proposal(&self) -> Option<Proposal> {
        self.proposer.reserve_next_chosen_proposal()
    }

    /// Creates a new proposal using the core proposer.
    fn create_proposal(&self) -> Option<Proposal> {
        let proposal = self.proposer.create_proposal();
        match &proposal {
            Some(p) => logger::log_debug(&format!(
                "[Proposer Agent] Created proposal: slot={}, ballot={}, value={:?}.",
                p.slot, p.ballot, p.value
            )),
            None => logger::log_debug(
                "[Proposer Agent] Failed to create proposal (not leader or no pending values).",
            ),
        }
        proposal
    }

    /// Synchronous prepare phase: broadcasts the prepare message, collects responses,
    fn prepare_phase(
        &self,
        slot: Slot,
        ballot: Ballot,
        initial_promises: Vec<Promise>,
    ) -> PrepareResult {
        match self.broadcast_prepare(slot, ballot) {
            CollectedResponses::EventDriven(result) => {
                // Event-driven mode
                self.start_prepare_round(); // Resets timer and sets set phase to pending.

                for promise in initial_promises {
                    self.insert_and_check_promises(promise, &self.node);
                }
                result
            }
            CollectedResponses::Synchronous(mut responses) => {
                // Combine any initial promises with the new responses.
                responses.extend(initial_promises);
                logger::log_debug(&format!(
                    "[Proposer Agent] Collected {} promise responses for slot {}.",
                    responses.len(),
                    slot
                ));
                let result = self.process_promises(slot, ballot, responses);
                self.handle_prepare_result(ballot, result.clone());
                result
            }
        }
    }

    /// Synchronous accept phase: broadcasts the accept message, collects responses,
    fn accept_phase(
        &self,
        value: Value,
        slot: Slot,
        ballot: Ballot,
        initial_accepts: Vec<Accepted>,
    ) -> AcceptResult {
        match self.broadcast_accept(value, slot, ballot) {
            CollectedResponses::EventDriven(result) => {
                // Event-driven mode
                for accepted in initial_accepts {
                    self.insert_and_check_accepted(accepted, &self.node);
                }
                result
            }
            CollectedResponses::Synchronous(mut responses) => {
                responses.extend(initial_accepts);
                logger::log_debug(&format!(
                    "[Proposer Agent] Collected {} accepted responses for slot {}.",
                    responses.len(),
                    slot
                ));
                let result = self.process_accepted(slot, responses);
                self.handle_accept_result(slot, result.clone());
                result
            }
        }
    }

    fn broadcast_learn(&self, learn: Learn) {
        let msg = NetworkMessage {
            sender: self.node.clone(),
            payload: MessagePayload::Learn(learn.clone()),
        };

        logger::log_info(&format!(
            "[Proposer Agent] Broadcasting LEARN to learners: slot={}, value={:?}.",
            learn.slot, learn.value
        ));

        network::send_message_forget(&self.learners, &msg);
    }

    fn mark_proposal_chosen(&self, slot: Slot) -> Option<Value> {
        self.proposer.mark_proposal_chosen(slot)
    }

    fn mark_proposal_finalized(&self, slot: Slot) -> Option<Value> {
        self.proposer.mark_proposal_finalized(slot)
    }

    fn proposals_to_accept(&self) -> Vec<Proposal> {
        let mut to_accept = Vec::new();
        for _ in 0..self.batch_size {
            if let Some(prop) = self.create_proposal() {
                // TESTING: FOR CREATING A PROPOSAL BUT NOT SENDING IT TO SIMULATE A MESSAGE LOSS
                // if prop.slot == self.test_skip_slot.get() {
                //     logger::log_warn(&format!(
                //         "[Proposer Agent] Skipping proposal for slot {} for testing purposes",
                //         prop.slot
                //     ));
                //     let num = rand::thread_rng().gen_range(10..30);
                //     self.test_skip_slot.set(self.test_skip_slot.get() + num);
                //     continue;
                // }
                to_accept.push(prop);
            } else {
                break;
            }
        }
        to_accept
    }

    fn learns_to_commit(&self) -> Vec<Learn> {
        let mut learns = Vec::new();
        for _ in 0..self.batch_size {
            if let Some(p) = self.reserve_next_chosen_proposal() {
                learns.push(Learn {
                    slot: p.slot,
                    value: p.value.clone(),
                });
            } else {
                break;
            }
        }
        learns
    }

    fn collect_client_responses(&self) -> Vec<ClientResponse> {
        let mut out = Vec::new();
        let mut queue = self.client_responses.borrow_mut();
        for _ in 0..self.batch_size {
            if let Some((_, resp)) = queue.pop_first() {
                out.push(resp);
            } else {
                break;
            }
        }
        out
    }

    fn process_executed(&self, executed: Executed) {
        if executed.adu > self.proposer.get_adu() {
            self.proposer.set_adu(executed.adu);
        }

        if !self.proposer.is_leader() {
            logger::log_debug(
                "[Proposer Agent] Executed received but not a leader; skipping client-response.",
            );
            return;
        }

        for r in &executed.results {
            if let Some(cmd_res) = r.cmd_result.clone() {
                let _ = self.mark_proposal_finalized(r.slot);

                let resp = ClientResponse {
                    client_id: r.value.client_id.clone(),
                    client_seq: r.value.client_seq,
                    success: true,
                    command_result: Some(cmd_res),
                };

                // Store the client response in the queue.
                self.client_responses
                    .borrow_mut()
                    .insert(r.slot, resp.clone());

                logger::log_info(&format!(
                    "[Proposer Agent] Created ClientResponse for slot {}",
                    r.slot
                ));
            } else {
                // No cmd result -> noop reenqueue proposal.
                // Set that specific slot + noop to finalized and reinsert into request queue
                // Send out accepted messages including the new enqueued proposal
                if let Some(value) = self.proposer.mark_proposal_finalized(r.slot) {
                    self.proposer.enqueue_client_request(&value);
                    for prop in self.proposals_to_accept() {
                        let accept_result = self.accept_phase(
                            prop.value.clone(),
                            prop.slot,
                            prop.ballot,
                            vec![], // no initial accepts
                        );
                        if let AcceptResult::IsEventDriven = accept_result {
                            // OK, keep going
                        } else {
                            logger::log_error(
                                "[Proposer Agent] Run loop is only supporting event driven.",
                            );
                            panic!("accept_commit only supports event-driven accepts");
                        }
                    }
                }
            }
        }
    }

    fn retry_learn(&self, slot: Slot) {
        // if it’s already been chosen/commit‐pending, just resend the Learn
        if let Some(chosen) = self.get_proposal_by_status(slot, ProposalStatus::CommitPending) {
            let learn = Learn {
                slot,
                value: chosen.value.clone(),
            };
            logger::log_warn(&format!(
                "[Proposer Agent] Retrying LEARN for slot {} → {:?}",
                slot, learn.value
            ));
            self.broadcast_learn(learn);
            return;
        }

        // otherwise if it’s still in flight, resend the Accept
        if let Some(in_flight) = self.get_proposal_by_status(slot, ProposalStatus::InFlight) {
            let accept = Accept {
                slot: in_flight.slot,
                ballot: in_flight.ballot,
                value: in_flight.value.clone(),
            };
            logger::log_warn(&format!(
                "[Proposer Agent] Retrying ACCEPT for slot {} → {:?}",
                slot, accept.value
            ));
            // we can either call our accept_phase helper or simply fire-and-forget:
            let _ = self.accept_phase(accept.value, accept.slot, accept.ballot, vec![]);
            return;
        }

        // TODO: Handle the case where a proposal has the Chosen status?
        // TODO: Meaning it was never retrieved by the reserve_next_chosen_proposal function and therefore never sent to learners.
        // Nothing known to retry
        logger::log_warn(&format!(
            "[Proposer Agent] No in-flight or commit-pending proposal for slot {}; nothing to retry",
            slot
        ));
    }

    fn handle_message(&self, message: NetworkMessage) -> NetworkMessage {
        logger::log_debug(&format!(
            "[Proposer Agent] Received network message: {:?}",
            message
        ));

        match message.payload {
            MessagePayload::Promise(payload) => {
                logger::log_debug(&format!(
                    "[Proposer Agent] Handling PROMISE: ballot={}, accepted={:?}",
                    payload.ballot, payload.accepted
                ));
                let sender = message.sender;

                // Insert promise and check if quorum might have been reached.
                if let Some((highest_ballot, promise_list)) =
                    self.insert_and_check_promises(payload.clone(), &sender)
                {
                    logger::log_debug(
                        "[Proposer Agent] Quorum reached for promises; processing asynchronously.",
                    );
                    let result =
                        self.process_promises(payload.slot.clone(), highest_ballot, promise_list);
                    self.handle_prepare_result(highest_ballot, result);
                }
                logger::log_debug(
                    "[Proposer Agent] Currently not enough replies, waiting for more.",
                );
                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore,
                }
            }
            MessagePayload::Accepted(payload) => {
                logger::log_debug(&format!(
                    "[Proposer Agent] Handling ACCEPTED: slot={}, ballot={}, success={}",
                    payload.slot, payload.ballot, payload.success
                ));
                let sender = message.sender;

                // Insert accepted and check if quorum might have been reached.
                if let Some((slot, accepted_list)) =
                    self.insert_and_check_accepted(payload, &sender)
                {
                    logger::log_debug(
                        "[Proposer Agent] Quorum reached for accepts; processing asynchronously.",
                    );
                    let result = self.process_accepted(slot, accepted_list);
                    self.handle_accept_result(slot, result);
                }

                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore,
                }
            }
            MessagePayload::Heartbeat(payload) => {
                logger::log_debug(&format!(
                    "[Proposer Agent] Handling HEARTBEAT: sender: {:?}, timestamp={}",
                    payload.sender, payload.timestamp
                ));
                // Simply echo the heartbeat payload.
                let response_payload = Heartbeat {
                    sender: self.node.clone(),
                    // timestamp = ... // TODO: Have a consistent way to define these?
                    timestamp: payload.timestamp,
                };
                self.failure_detector
                    .heartbeat(payload.sender.clone().node_id);
                // TODO: Have a dedicated heartbeat ack payload type?
                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Heartbeat(response_payload),
                }
            }
            MessagePayload::RetryLearn(slot) => {
                logger::log_warn(&format!(
                    "[Proposer Agent] Handling LEARN RETRY: slots={:?}",
                    slot
                ));
                if !self.proposer.is_leader() {
                    logger::log_warn(&format!(
                        "[Proposer Agent] RetryLearn for slots {:?} received but not a leader. Ignoring",
                        slot
                    ));
                } else {
                    self.retry_learn(slot);
                }
                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore,
                }
            }
            MessagePayload::Executed(executed) => {
                self.process_executed(executed);

                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore,
                }
            }
            other_message => {
                logger::log_warn(&format!(
                    "[Proposer Agent] Received irrelevant message type: {:?}",
                    other_message
                ));
                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore,
                }
            }
        }
    }

    fn is_leader(&self) -> bool {
        self.proposer.is_leader()
    }

    fn become_leader(&self) -> bool {
        self.proposer.become_leader()
    }

    fn resign_leader(&self) -> bool {
        self.proposer.resign_leader()
    }

    fn start_leader_loop(&self) {
        self.set_phase(PaxosPhase::PrepareSend);
    }

    fn check_prepare_timeout(&self) -> bool {
        if self.phase.get() == PaxosPhase::PreparePending {
            if let Some(start) = self.last_prepare_start.get() {
                if start.elapsed() >= Duration::from_millis(self.config.prepare_timeout) {
                    logger::log_warn(
                        "[Proposer Agent] Prepare phase timed out. Reverting to PrepareSend.",
                    );
                    self.advance_phase(Some(false));
                    self.last_prepare_start.set(None);
                    return true;
                }
            }
        }
        false
    }

    // TODO: Reduce the repeating code from the alternative run function?
    fn run_paxos_loop(&self) -> Option<Vec<ClientResponse>> {
        // Ticker called from host (when running modular models)
        match self.phase.get() {
            PaxosPhase::Start => {
                logger::log_debug("[Proposer Agent] Run loop: Start phase.");
                if self.proposer.is_leader() {
                    self.start_leader_loop();
                }
                None
            }
            PaxosPhase::PrepareSend => {
                logger::log_debug("[Proposer Agent] Run loop: PrepareSend phase.");
                let slot = self.proposer.get_adu() + 1;
                let ballot = self.proposer.get_current_ballot();

                let prepare_result = self.prepare_phase(slot, ballot, vec![]);
                if let PrepareResult::IsEventDriven = prepare_result {
                } else {
                    logger::log_error("[Proposer Agent] Run loop is only supporting event driven.");
                    panic!("Lol");
                }
                None
            }
            PaxosPhase::PreparePending => {
                logger::log_debug("[Proposer Agent] Run loop: PreparePending phase.");
                // In this phase we wait for responses. If a timeout occurs, we revert back.
                self.check_prepare_timeout();
                None
            }

            // Currently process up to 10 send accept, send learn, respond to client in one tick
            // TODO: Could actually batch here, not send out on at a time but rather a batch of 10
            PaxosPhase::AcceptCommit => {
                logger::log_debug("[Proposer Agent] Run loop: AcceptCommit phase.");
                return self.accept_commit();
                //* The event driven design handles the rest, unless we want to move distinguished learner logic here. No :) */
            }
            PaxosPhase::Stop => None,  // TODO
            PaxosPhase::Crash => None, // TODO
        }
    }

    // TODO : not used properly by any of the agents
    fn failure_service(&self) {
        let new_lead = self.failure_detector.checker();
        if let Some(leader) = new_lead {
            logger::log_warn(&format!("Leader {} change initiated. New leader", &leader));
            if leader == self.node.node_id {
                self.become_leader();
            }
        }
    }
}
