use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, VecDeque};

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "proposer-world",
        // generate_unused_types: true,
        additional_derives: [PartialEq, Clone],
    });
}

bindings::export!(MyProposer with_types_in bindings);

use bindings::exports::paxos::default::proposer::{Guest, GuestProposerResource, ProposerState};
use bindings::paxos::default::logger;
use bindings::paxos::default::paxos_types::{
    Accepted, Ballot, ClientRequest, PValue, Promise, Proposal, Slot, Value,
};
use bindings::paxos::default::proposer_types::{AcceptResult, PrepareResult};

pub struct MyProposer;

impl Guest for MyProposer {
    type ProposerResource = MyProposerResource;
}

pub struct MyProposerResource {
    is_leader: Cell<bool>,
    num_acceptors: u64,
    ballot_delta: Ballot,

    current_slot: Cell<Slot>,
    current_ballot: Cell<Ballot>,
    // adu: Cell<Ballot>,

    // New values submitted by clients.
    pending_client_requests: RefCell<VecDeque<ClientRequest>>,
    // Values that must be proposed again after a failed accept phase.
    prioritized_values: RefCell<VecDeque<PValue>>, // TODO: Have it indexed by slot instead to enforce slot -> value mapping for re-proposals?
    // Proposals currently in flight, indexed by slot.
    in_flight_proposals: RefCell<HashMap<Slot, Proposal>>,

    accepted_proposals: RefCell<BTreeMap<Slot, Proposal>>, // Adu, just per slot and with the proposal
                                                           // TODO: Move the state and handling of in_flight_accepted on the proposer agent here?
}

impl MyProposerResource {
    fn quorum(&self) -> u64 {
        return (self.num_acceptors / 2) + 1;
    }

    // fn advance_adu(&self) -> Slot {
    //     self.adu.set(self.adu.get() + 1);
    //     self.adu.get()
    // }

    fn get_next_slot(&self) -> Slot {
        let next_slot = self.current_slot.get() + 1;
        self.current_slot.set(next_slot);
        next_slot
    }

    fn next_value(&self) -> Option<Value> {
        if let Some(val) = self.prioritized_values.borrow_mut().pop_front() {
            Some(val.value?) // TODO: Enforce the usage of same slot, should be the same though?
        } else if let Some(req) = self.pending_client_requests.borrow_mut().pop_front() {
            Some(req.value)
        } else {
            None
        }
    }

    /// Collect all accepted values for slots higher than min_slot,
    /// ensuring that only those slots that have reached quorum are kept.
    /// Also fills in missing slots with no-op PValue entries.
    fn collect_accepted_values_with_quorum(
        &self,
        min_slot: Slot,
        promises: &[Promise],
    ) -> Vec<PValue> {
        let quorum = self.quorum() as usize;
        let ballot = self.current_ballot.get();

        // Map each slot to (count, best accepted value)
        let mut slot_map: HashMap<Slot, (usize, PValue)> = HashMap::new();

        for promise in promises {
            for accepted in &promise.accepted {
                // Consider only accepted values for slots higher than min_slot.
                if accepted.slot > min_slot {
                    // Get or insert the current entry for the slot.
                    let entry = slot_map
                        .entry(accepted.slot)
                        .or_insert((0, accepted.clone()));
                    entry.0 += 1; // The counter
                    // Choose the accepted value with the highest ballot.
                    if accepted.ballot > entry.1.ballot {
                        entry.1 = accepted.clone();
                    }
                }
            }
        }

        // Collect only the accepted values that meet the quorum requirement.
        let mut accepted_with_quorum: Vec<PValue> = slot_map
            .into_iter()
            .filter_map(|(_slot, (count, accepted))| {
                if count >= quorum {
                    Some(accepted)
                } else {
                    None
                }
            })
            .collect();

        accepted_with_quorum.sort_by_key(|accepted| accepted.slot);

        // Fill in missing slots with no-op PValue entries.
        let mut complete_values = Vec::new();
        let mut expected_slot = min_slot + 1;
        for accepted in accepted_with_quorum.into_iter() {
            // For any missing slot, insert a no-op value.
            let cur_slot = accepted.slot;
            while expected_slot < cur_slot {
                complete_values.push(PValue {
                    slot: expected_slot,
                    ballot: ballot,
                    value: Some(Value {
                        is_noop: true,
                        command: None,
                    }), // TODO: No-op. Do this another place?
                });
                expected_slot += 1;
            }
            complete_values.push(accepted);
            expected_slot = cur_slot + 1;
        }

        complete_values
    }
}

// TODO: Make the proposer also increase ballot if phase 1 fails and try again

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
            ballot_delta: init_ballot,

            current_slot: Cell::new(0),
            current_ballot: Cell::new(init_ballot),
            // adu: Cell::new(0),
            pending_client_requests: RefCell::new(VecDeque::new()),
            prioritized_values: RefCell::new(VecDeque::new()),
            in_flight_proposals: RefCell::new(HashMap::new()),
            // in_flight_accepted: RefCell::new(BTreeMap::new()),
            accepted_proposals: RefCell::new(BTreeMap::new()),
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
            in_flight: self
                .in_flight_proposals
                .borrow()
                .values()
                .cloned()
                .collect(),
            last_proposal: self.in_flight_proposals.borrow().values().last().cloned(),
        }
    }

    /// Enqueues a client request for later proposal creation.
    fn enqueue_client_request(&self, req: ClientRequest) -> bool {
        self.pending_client_requests.borrow_mut().push_back(req);
        logger::log_debug("[Core Proposer] Enqueued client request.");
        true
    }

    fn get_current_slot(&self) -> Slot {
        self.current_slot.get()
    }

    fn get_current_ballot(&self) -> Slot {
        self.current_ballot.get()
    }

    fn get_in_flight_proposal(&self, slot: Slot) -> Option<Proposal> {
        return self.in_flight_proposals.borrow().get(&slot).cloned();
    }
    // TODO: Merge these by having a "accepted" field on the Proposal itself?

    fn get_accepted_proposal(&self, slot: Slot) -> Option<Proposal> {
        return self.accepted_proposals.borrow().get(&slot).cloned();
    }

    /// Returns Some(proposal) if created; otherwise, None.
    fn create_proposal(&self) -> Option<Proposal> {
        if !self.is_leader.get() {
            logger::log_warn("[Core Proposer] Not leader; proposal creation aborted.");
            return None;
        }

        let value = match self.next_value() {
            Some(val) => val,
            None => {
                logger::log_info("[Core Proposer] No pending values available for proposal.");
                return None;
            }
        };

        let slot = self.get_next_slot();

        let prop = Proposal {
            ballot: self.get_current_ballot(),
            slot,
            value,
        };

        // Create an in-flight entry with proposal
        self.in_flight_proposals
            .borrow_mut()
            .insert(slot, prop.clone());

        logger::log_info(&format!(
            "[Core Proposer] Created proposal: ballot = {}, slot = {}, value = {:?}.",
            prop.ballot, prop.slot, prop.value,
        ));

        Some(prop)
    }

    /// Processes promise responses and returns a prepare result.
    /// Chooses the highest accepted value if available.
    fn process_prepare(&self, min_slot: Slot, promises: Vec<Promise>) -> PrepareResult {
        let quorum: usize = self.quorum() as usize;
        let _current_slot = self.current_slot.get();
        let current_ballot = self.current_ballot.get();

        // Filter promises for the current ballot.
        let valid_promises: Vec<&Promise> = promises
            .iter()
            .filter(|p| p.ballot == current_ballot)
            .collect();

        if valid_promises.len() < quorum {
            logger::log_warn(&format!(
                "[Core Proposer] Prepare phase failed: only {} valid promises received (quorum required: {}).",
                valid_promises.len(),
                quorum
            ));
            return PrepareResult::QuorumFailure;
        }
        logger::log_debug(&format!(
            "[Core Proposer] Prepare phase: received {}/{} valid promises (quorum required: {}).",
            valid_promises.len(),
            self.num_acceptors,
            quorum
        ));

        // TODO: use "current_slot" instead of having the "min_slot" argument?
        let accepted_values = self.collect_accepted_values_with_quorum(min_slot, &promises);

        // Save the accepted values into the pending accepted values queue.
        self.prioritized_values.borrow_mut().extend(accepted_values);

        logger::log_info(&format!(
            "[Core Proposer] Prepare phase succeeded with ballot {}.",
            current_ballot
        ));
        // TODO: Enforce correct usage by setting a "phase" flag to "phase-2" here?
        PrepareResult::Success
    }

    /// Processes accepted responses and returns an accept result.
    /// Compares the count of successful accepts against the quorum.
    fn process_accept(&self, slot: Slot, accepts: Vec<Accepted>) -> AcceptResult {
        let prop = match self.get_in_flight_proposal(slot) {
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

        // TODO: Fix this
        // if count >= self.num_acceptors as usize {
        //     logger::log_debug(&format!(
        //         "[Core Proposer] Accept phase failed for slot {}. All acceptors answered.",
        //         slot
        //     ));
        //     self.prioritized_values.borrow_mut().push_back(PValue {
        //         // TODO: Add back to queue as just a Value?
        //         slot: slot,
        //         ballot: self.current_ballot.get(),
        //         value: Some(prop.value.clone()),
        //     });
        // }

        if count < self.quorum() as usize {
            logger::log_debug(&format!(
                "[Core Proposer] Accept phase failed for slot {}: {} acceptances received (quorum is {}).",
                slot,
                count,
                self.quorum()
            ));
            AcceptResult::QuorumFailure
        } else {
            logger::log_info(&format!(
                "[Core Proposer] Accept phase succeeded for slot {} with {} acceptances.",
                prop.slot, count
            ));
            if self.get_in_flight_proposal(slot).is_none() {
                return AcceptResult::MissingProposal;
            }

            AcceptResult::Accepted(count as u64)
        }
    }

    /// Finalizes a proposal
    fn finalize_proposal(&self, slot: Slot) -> Option<Value> {
        let prop = match self.get_in_flight_proposal(slot) {
            Some(p) => p,
            None => {
                logger::log_error(&format!(
                    "[Core Proposer] In-flight proposal for slot {} not found during finalization.",
                    slot
                ));
                return None;
            }
        };
        self.in_flight_proposals.borrow_mut().remove(&slot);
        self.accepted_proposals
            .borrow_mut()
            .insert(slot, prop.clone()); // TODO: Merge these by having a "accepted" field on the Proposal itself?

        logger::log_info(&format!(
            "[Core Proposer] Finalized accepted slot {} with value {:?}.",
            slot, prop.value
        ));
        Some(prop.value)
    }

    fn increase_ballot(&self) -> Ballot {
        let new_ballot = self.current_ballot.get() + self.ballot_delta;
        self.current_ballot.set(new_ballot);
        logger::log_info(&format!(
            "Core Proposer] Increased current ballot to: {}",
            new_ballot
        ));
        new_ballot
    }

    fn is_leader(&self) -> bool {
        self.is_leader.get()
    }

    // TODO: handle the process when leader change properly

    /// Increments the ballot and sets the leader flag.
    /// Returns true if leadership is acquired.
    fn become_leader(&self) -> bool {
        if self.is_leader() {
            logger::log_warn("[Core Proposer] Already leader; cannot become leader again.");
            false
        } else {
            self.increase_ballot();
            self.is_leader.set(true);
            logger::log_info("[Core Proposer] Leadership acquired.");
            true
        }
    }

    /// Clears the leader flag.
    /// Returns true if leadership was successfully resigned.
    fn resign_leader(&self) -> bool {
        if self.is_leader() {
            self.is_leader.set(false);
            logger::log_info("[Core Proposer] Leadership resigned.");
            true
        } else {
            logger::log_warn("[Core Proposer] Not leader; resign operation aborted.");
            false
        }
    }
}
