use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
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
use bindings::paxos::default::network_client::NetworkClientResource;
use bindings::paxos::default::network_types::{MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{
    Accept, Accepted, Ballot, ClientResponse, CmdResult, Executed, Learn, Node, PaxosPhase,
    PaxosRole, Prepare, Promise, Proposal, RunConfig, Slot, Value,
};
use bindings::paxos::default::proposer_types::{AcceptResult, PrepareResult, ProposalStatus};
use bindings::paxos::default::{logger, network_client, proposer::ProposerResource};

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
    all_nodes: Vec<Node>,
    network_client: Arc<network_client::NetworkClientResource>,

    // A mapping from Ballot to unique promises per node_id.
    promises: RefCell<BTreeMap<Ballot, HashMap<u64, Promise>>>,
    in_flight_accepted: RefCell<BTreeMap<Slot, HashMap<u64, Accepted>>>, //* Per slot accepted per sender */
    client_responses: RefCell<BTreeMap<Slot, ClientResponse>>,

    last_prepare_start: Cell<Option<Instant>>,
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

        logger::log_info(&format!(
            "[Proposer Agent] Received promise for ballot {}, slot {} from {}.",
            promise.ballot, promise.slot, sender.node_id
        ));

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

        logger::log_info(&format!(
            "[Proposer Agent] Broadcasting PREPARE: slot={}, ballot={}.",
            slot, ballot
        ));

        if !self.config.is_event_driven {
            // In synchronous mode, send and collect responses.
            let responses = self.network_client.send_message(&self.acceptors, &msg);
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
            self.network_client
                .send_message_forget(&self.acceptors, &msg);
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
            let responses = self.network_client.send_message(&self.acceptors, &msg);
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
            self.network_client
                .send_message_forget(&self.acceptors, &msg);
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
            .iter()
            .filter(|x| matches!(x.role, PaxosRole::Acceptor | PaxosRole::Coordinator))
            .cloned()
            .collect();

        let learners: Vec<_> = nodes
            .iter()
            .filter(|x| matches!(x.role, PaxosRole::Learner | PaxosRole::Coordinator))
            .cloned()
            .collect();

        let self_is_coordinator = node.role == PaxosRole::Coordinator;
        let num_acceptors = acceptors.len() as u64 + self_is_coordinator as u64;
        let num_learners = learners.len() as u64 + self_is_coordinator as u64;

        let init_ballot = node.node_id;
        let proposer = Arc::new(ProposerResource::new(
            is_leader,
            num_acceptors,
            init_ballot,
            &node.node_id.to_string(),
            config,
        ));

        let network_client = Arc::new(NetworkClientResource::new());

        match proposer.load_state() {
            Ok(_) => logger::log_info("[Proposer Agent] Loaded state successfully."),
            Err(e) => logger::log_warn(&format!(
                "[Proposer Agent] Failed to load state. Ignore if first startup: {}",
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
            network_client,

            client_responses: RefCell::new(BTreeMap::new()),
            all_nodes: nodes,
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

        self.network_client
            .send_message_forget(&self.learners, &msg);
    }

    fn mark_proposal_chosen(&self, slot: Slot) -> Option<Value> {
        self.proposer.mark_proposal_chosen(slot)
    }

    fn mark_proposal_finalized(&self, slot: Slot) -> Option<Value> {
        self.proposer.mark_proposal_finalized(slot)
    }

    fn proposals_to_accept(&self) -> Vec<Proposal> {
        let mut to_accept = Vec::new();
        for _ in 0..self.config.batch_size {
            if let Some(prop) = self.create_proposal() {
                to_accept.push(prop);
            } else {
                break;
            }
        }
        to_accept
    }

    fn learns_to_commit(&self) -> Vec<Learn> {
        let mut learns = Vec::new();
        for _ in 0..self.config.batch_size {
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
        for _ in 0..self.config.batch_size {
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
            // TODO: Put this into config
            // should also have a better way to crash the node since adu with batch size
            // wont necessarily be incremented by 1
            // But do not know where its best to do this, since crashing at a specific slot when sending out
            // would mean crash on the recovered node aswell for that slot
            // if executed.adu == 1001 && self.proposer.is_leader() {
            //     panic!("Testing panic recovery");
            // }
        }

        if !self.proposer.is_leader() {
            logger::log_debug(
                "[Proposer Agent] Executed received but not a leader; skipping client-response.",
            );
            return;
        }

        for result in &executed.results {
            if let Some(value) = self.proposer.mark_proposal_finalized(result.slot) {
                let resp = ClientResponse {
                    client_id: value.client_id.clone(),
                    client_seq: value.client_seq,
                    success: result.success, // No-op, or executed a valid operation.
                    command_result: result.cmd_result.clone(),
                };
                self.client_responses
                    .borrow_mut()
                    .insert(result.slot, resp.clone());

                match &result.cmd_result {
                    CmdResult::NoOp => {
                        logger::log_warn(&format!(
                            "[Proposer Agent] Slot {} was no-op; responding with failure",
                            result.slot
                        ));
                        // only retry if there was a original valid value on the slot
                        if value.command.is_some() {
                            self.proposer.enqueue_prioritized_request(&value);
                            logger::log_info(&format!(
                                "[Proposer Agent] Re-enqueued client request for slot {}",
                                result.slot
                            ));
                        }
                    }
                    CmdResult::CmdValue(cmd_result_value) if result.success => {
                        // real command, and Paxos said it succeeded
                        if cmd_result_value.is_some() {
                            logger::log_info(&format!(
                                "[Proposer Agent] Slot {}: command succeeded; client response queued",
                                result.slot
                            ));
                        } else {
                            logger::log_info(&format!(
                                "[Proposer Agent] Slot {}: command succeeded with no return value; response queued",
                                result.slot
                            ));
                        }
                    }
                    CmdResult::CmdValue(_) => {
                        // real command, but Paxos said it failed. //* Can't happen? */
                        logger::log_warn(&format!(
                            "[Proposer Agent] Slot {}: command failed; client response queued",
                            result.slot
                        ));
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
                let ballot = self.proposer.increase_ballot();

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

            PaxosPhase::AcceptCommit => {
                logger::log_debug("[Proposer Agent] Run loop: AcceptCommit phase.");
                self.accept_commit()
                //* The event driven design handles the rest, unless we want to move distinguished learner logic here. No :) */
            }
            PaxosPhase::Stop => None,  // TODO
            PaxosPhase::Crash => None, // TODO
        }
    }

    fn send_heartbeat(&self) {
        let heartbeat_msg = NetworkMessage {
            sender: self.node.clone(),
            payload: MessagePayload::Heartbeat,
        };

        logger::log_info(&format!(
            "[Proposer Agent] Sending heartbeat to all nodes: {:?}",
            self.all_nodes
        ));
        self.network_client
            .send_message_forget(&self.all_nodes, &heartbeat_msg);
    }
}
