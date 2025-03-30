use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "proposer-world",
        // generate_unused_types: true,
        additional_derives: [PartialEq],
    });
}

bindings::export!(MyProposer with_types_in bindings);

use bindings::exports::paxos::default::proposer::{Guest, GuestProposerResource, ProposerState};
use bindings::paxos::default::logger;
use bindings::paxos::default::paxos_types::{
    Accepted, Ballot, ClientRequest, Promise, Proposal, Slot, Value, Accept, Prepare, Learn
};
use bindings::paxos::default::proposer_types::{AcceptResult, PrepareOutcome, PrepareResult};

pub struct MyProposer;

impl Guest for MyProposer {
    type ProposerResource = MyProposerResource;
}

// Maintains current state, pending proposals, and in-flight proposals.
pub struct MyProposerResource {
    is_leader: Cell<bool>,
    num_acceptors: u64,

    current_slot: Cell<Slot>,
    current_ballot: Cell<Ballot>,

    pending_client_requests: RefCell<VecDeque<ClientRequest>>,
    in_flight: RefCell<HashMap<Slot, Proposal>>, //* Per instance (slot) proposals */

    in_flight_accepts: RefCell<VecDeque<Accept>>,
    adu: Cell<u64>,
    
    promises: RefCell<Vec<Promise>>,
    learns: RefCell<HashMap<u64, Vec<Learn>>>
}

impl MyProposerResource {
    fn quorum(&self) -> u64 {
        return (self.num_acceptors / 2) + 1;
    }
}

// TODO: Remove or improve upon the redundant return types

impl GuestProposerResource for MyProposerResource {
    fn new(is_leader: bool, num_acceptors: u64, init_ballot: Ballot) -> Self {
        logger::log_info(&format!(
            "[Core Proposer] Initialized as {} node with {} acceptors and initial ballot {}.",
            if is_leader { "leader" } else { "normal" },
            num_acceptors,
            init_ballot
        ));

        Self {
            is_leader: Cell::new(is_leader),
            num_acceptors,

            current_slot: Cell::new(0),
            current_ballot: Cell::new(init_ballot),

            pending_client_requests: RefCell::new(VecDeque::new()),
            in_flight: RefCell::new(HashMap::new()),

            in_flight_accepts: RefCell::new(VecDeque::new()),
            adu: Cell::new(0),
            
            promises: RefCell::new(Vec::new()),
            learns: RefCell::new(HashMap::new()),
        }
    }

    fn get_state(&self) -> ProposerState {
        ProposerState {
            is_leader: self.is_leader.get(),
            num_acceptors: self.num_acceptors,

            current_slot: self.current_slot.get(),
            current_ballot: self.current_ballot.get(),

            pending_client_requests: self
                .pending_client_requests
                .borrow()
                .iter()
                .cloned()
                .collect(),
            in_flight: self.in_flight.borrow().values().cloned().collect(),
            last_proposal: self.in_flight.borrow().values().last().cloned(),
        }
    }

    /// Enqueues a client request for later proposal creation.
    fn enqueue_client_request(&self, req: ClientRequest) -> bool {
        self.pending_client_requests.borrow_mut().push_back(req);
        logger::log_debug("[Core Proposer] Enqueued client request.");
        true
    }

    fn get_proposal(&self, slot: Slot) -> Option<Proposal> {
        return self.in_flight.borrow().get(&slot).cloned();
    }

    /// Creates the next proposal if one client_request is pending and the node is leader.
    /// Returns Some(proposal) if created; otherwise, None.
    fn create_proposal(&self) -> Option<Proposal> {
        if !self.is_leader.get() {
            logger::log_warn("[Core Proposer] Not leader; proposal creation aborted.");
            return None;
        }

        let client_req = match self.pending_client_requests.borrow_mut().pop_front() {
            Some(cr) => cr,
            None => {
                logger::log_info("[Core Proposer] No pending client requests available.");
                return None;
            }
        };

        // Assign the next available slot.
        let slot = self.current_slot.get();
        self.current_slot.set(slot + 1);

        let prop = Proposal {
            ballot: self.current_ballot.get(),
            slot,
            client_request: client_req,
        };

        // Create an in-flight entry with proposal
        self.in_flight.borrow_mut().insert(slot, prop.clone());

        logger::log_info(&format!(
            "[Core Proposer] Created proposal: ballot = {:?}, slot = {}, value = '{}'.",
            prop.client_request.value, prop.slot, prop.ballot,
        ));

        Some(prop)
    }

    /// Processes promise responses and returns a prepare result.
    /// Chooses the highest accepted value if available.
    fn process_prepare(&self, slot: Slot, promises: Vec<Promise>) -> PrepareResult {
        let prop = match self.get_proposal(slot) {
            Some(p) => p,
            None => {
                logger::log_error(&format!(
                    "[Core Proposer] In-flight proposal for slot {} not found during prepare phase.",
                    slot
                ));
                return PrepareResult::MissingProposal;
            }
        };

        logger::log_debug(&format!(
            "[Core Proposer] Received promises: {:?}",
            promises
        ));

        let mut count = 0;
        let mut highest_accepted_ballot = 0;
        let mut chosen_value: Option<Value> = None;

        // Iterate over all accepted p-values for the proposal, filtering by ballot and slot.
        for pv in promises
            .iter()
            .filter(|prom| prom.ballot == prop.ballot)
            .flat_map(|prom| prom.accepted.iter())
            .filter(|pv| pv.slot == prop.slot)
        {
            count += 1;
            if pv.ballot > highest_accepted_ballot {
                highest_accepted_ballot = pv.ballot;
                chosen_value = pv.value.clone();
            }
        }

        if count < self.quorum() {
            logger::log_warn(&format!(
                "[Core Proposer] Prepare phase failed: {} accepted proposals received (quorum is {}).",
                count,
                self.quorum()
            ));
            return PrepareResult::QuorumFailure;
        }

        // If no accepted value was found, use the original client's value.
        let outcome = if let Some(val) = chosen_value {
            PrepareOutcome {
                chosen_value: val,
                is_original: false,
            }
        } else {
            PrepareOutcome {
                chosen_value: prop.client_request.value.clone(),
                is_original: true,
            }
        };

        logger::log_info(&format!(
            "[Core Proposer] Prepare phase succeeded for slot {} (is_original: {}).",
            prop.slot, outcome.is_original
        ));
        PrepareResult::Outcome(outcome)
    }

    /// Processes accepted responses and returns an accept result.
    /// Compares the count of successful accepts against the quorum.
    fn process_accept(&self, slot: Slot, accepts: Vec<Accepted>) -> AcceptResult {
        let prop = match self.get_proposal(slot) {
            Some(p) => p,
            None => {
                logger::log_error(&format!(
                    "[Core Proposer] In-flight proposal for slot {} not found during accept phase.",
                    slot
                ));
                return AcceptResult::MissingProposal;
            }
        };

        // Count all accepted responses that match the proposal's slot, ballot, and that indicate success.
        let count = accepts
            .iter()
            .filter(|a| a.slot == prop.slot && a.ballot == prop.ballot && a.success)
            .count();

        if count < self.quorum() as usize {
            logger::log_warn(&format!(
                "[Core Proposer] Accept phase failed: {} acceptances received (quorum is {}).",
                count,
                self.quorum()
            ));
            AcceptResult::QuorumFailure
        } else {
            logger::log_info(&format!(
                "[Core Proposer] Accept phase succeeded for slot {} with {} acceptances.",
                prop.slot, count
            ));
            AcceptResult::Accepted(count as u64)
        }
    }

    /// Finalizes a proposal using the chosen value.
    /// Re-enqueues the original client request if the value changed.
    fn finalize_proposal(&self, slot: Slot, chosen_value: Value) -> bool {
        let prop = match self.get_proposal(slot) {
            Some(p) => p,
            None => {
                logger::log_error(&format!(
                    "[Core Proposer] In-flight proposal for slot {} not found during finalization.",
                    slot
                ));
                return false;
            }
        };
        let slot = prop.slot;
        self.in_flight.borrow_mut().remove(&slot);

        // TODO: More explicit Value equality check?
        if prop.client_request.value != chosen_value {
            logger::log_info(&format!(
                "[Core Proposer] Finalized slot {}: chosen value {:?} differs from original {:?}. Re-enqueuing client request.",
                slot, chosen_value, prop.client_request.value
            ));
            self.pending_client_requests
                .borrow_mut()
                .push_back(prop.client_request);
        } else {
            logger::log_info(&format!(
                "[Core Proposer] Finalized slot {:?} with original value {:?}.",
                slot, chosen_value
            ));
        }
        true
    }

    // TODO: handle the process when leader change properly

    /// Increments the ballot and sets the leader flag.
    /// Returns true if leadership is acquired.
    fn become_leader(&self) -> bool {
        if self.is_leader.get() {
            logger::log_warn("[Core Proposer] Already leader; cannot become leader again.");
            false
        } else {
            let new_ballot = self.current_ballot.get() + 1;
            self.current_ballot.set(new_ballot);
            self.is_leader.set(true);
            logger::log_info(&format!(
                "[Core Proposer] Leadership acquired with new ballot {}.",
                new_ballot
            ));
            true
        }
    }

    /// Clears the leader flag.
    /// Returns true if leadership was successfully resigned.
    fn resign_leader(&self) -> bool {
        if self.is_leader.get() {
            self.is_leader.set(false);
            logger::log_info("[Core Proposer] Leadership resigned.");
            true
        } else {
            logger::log_warn("[Core Proposer] Not leader; resign operation aborted.");
            false
        }
    }


    fn create_prepare(&self) -> Prepare {
        let prepare = Prepare {
            slot: self.adu.get() + 1,
            ballot: self.current_ballot.get()
        };
        prepare
    }

    fn register_promise(&self, promise: Promise) -> bool {
        if promise.ballot != self.current_ballot.get() {
            return  false;
        }
        self.promises.borrow_mut().push(promise);
        true
    }

    // TODO : should process similar to PrepareQF which returns all slots > prepare.slot acceptors have accepted
    // TODO: + also the last logic inside prpoposer runphaseone which appends them to a priority queue to get new leader uptospeed
    fn process_promises(&self) -> bool {
        if self.promises.borrow().len() < self.quorum() as usize {
            logger::log_info("[Core Proposer] No quorum of promises");
            return false;
        }
        true
    }

    fn register_learn(&self, learn: Learn) -> bool {
        self.learns
            .borrow_mut()
            .entry(learn.slot)
            .or_default()
            .push(learn);
        true
    }


    fn create_accept(&self) -> Option<Accept> {
        // if !self.is_leader.get() {
        //     logger::log_warn("[Core Proposer] Not leader; proposal creation aborted.");
        //     return None;
        // }

        // Should take have priority queue for handling the leader change thingy

        let client_req = match self.pending_client_requests.borrow_mut().pop_front() {
            Some(cr) => cr,
            None => {
                logger::log_info("[Core Proposer] No pending client requests available.");
                return None;
            }
        };

        // Assign the next available slot.
        let slot = self.current_slot.get();
        self.current_slot.set(slot + 1);

        let prop = Accept {
            ballot: self.current_ballot.get(),
            slot,
            value: client_req.value,
        };

        // Create an in-flight entry with accept
        self.in_flight_accepts.borrow_mut().push_back(prop.clone());

        logger::log_info(&format!(
            "[Core Proposer] Created proposal: ballot = {}, slot = {}, value = '{:?}'.",
            prop.ballot, prop.slot, prop.value,
        ));

        Some(prop)
    }

    
    // TODO : should process similar to AcceptQF
    fn process_learns(&self) -> Option<Learn> {
        let mut queue = self.in_flight_accepts.borrow_mut();
        if let Some(next_accept) = queue.pop_front() {
            logger::log_info(&format!(
                "[Core Proposer] Checking for learns for slot = {}",
                next_accept.slot
            ));
            
            let learns_map = self.learns.borrow();
            if let Some(learns_vec) = learns_map.get(&next_accept.slot) {
                if learns_vec.len() < self.quorum() as usize {
                    // Push back since quorum not met
                    logger::log_info(&format!(
                        "[Core Proposer] Quorum not reached yet for slot = {}",
                        next_accept.slot
                    ));
                    drop(queue);
                    self.in_flight_accepts.borrow_mut().push_front(next_accept);
                    return None;
                }
                return learns_vec.first().cloned();
            } else {
                logger::log_info(&format!(
                    "[Core Proposer] No learns yet for slot = {}",
                    next_accept.slot
                ));
                drop(queue);
                self.in_flight_accepts.borrow_mut().push_front(next_accept);
            }
        }
        None
    }

    fn advance_adu(&self) {
        self.adu.set(self.adu.get() + 1);
    }
}
