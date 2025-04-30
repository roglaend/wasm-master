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
    Accepted, Ballot, PValue, Promise, Proposal, Slot, Value,
};
use bindings::paxos::default::proposer_types::{
    AcceptResult, PrepareResult, ProposalEntry, ProposalStatus,
};

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

    pending_client_requests: RefCell<VecDeque<Value>>,
    prioritized_values: RefCell<BTreeMap<Slot, Option<Value>>>,
    proposals: RefCell<BTreeMap<Slot, ProposalEntry>>,
}

impl MyProposerResource {
    fn quorum(&self) -> u64 {
        return (self.num_acceptors / 2) + 1;
    }

    fn get_next_slot(&self) -> Slot {
        let next_slot = self.current_slot.get() + 1;
        self.current_slot.set(next_slot);
        next_slot
    }

    fn next_value(&self) -> Option<Value> {
        if let Some((_, value)) = self.prioritized_values.borrow_mut().pop_first() {
            value
        } else if let Some(req) = self.pending_client_requests.borrow_mut().pop_front() {
            Some(req)
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

        self.current_slot.set(min_slot - 1);
        // Map each slot to (count, best accepted value)
        let mut slot_map: HashMap<Slot, (usize, PValue)> = HashMap::new();

        for promise in promises {
            for accepted in &promise.accepted {
                // Consider only accepted values for slots higher than min_slot.
                if accepted.slot >= min_slot {
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
                    ballot,
                    value: None,
                });
                expected_slot += 1;
            }
            complete_values.push(accepted);
            expected_slot = cur_slot + 1;
        }

        complete_values
    }

    fn update_proposal_status(&self, slot: Slot, new_status: ProposalStatus) -> Option<Value> {
        let mut map = self.proposals.borrow_mut();
        if let Some(entry) = map.get_mut(&slot) {
            let old_value = entry.proposal.value.clone();
            entry.status = new_status;
            Some(old_value)
        } else {
            logger::log_error(&format!(
                "[Proposer] Tried to update status for unknown slot {}.",
                slot
            ));
            None
        }
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
            ballot_delta: num_acceptors, // TODO: This right?

            current_slot: Cell::new(0),
            current_ballot: Cell::new(init_ballot),

            pending_client_requests: RefCell::new(VecDeque::new()),
            prioritized_values: RefCell::new(BTreeMap::new()),
            proposals: RefCell::new(BTreeMap::new()),
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
            proposals: self.proposals.borrow().values().cloned().collect(),
        }
    }

    /// Enqueues a client request for later proposal creation.
    fn enqueue_client_request(&self, req: Value) -> bool {
        self.pending_client_requests.borrow_mut().push_back(req);
        logger::log_debug("[Core Proposer] Enqueued client request.");

        let queue = self.pending_client_requests.borrow();
        logger::log_info(&format!(
            "[Core Proposer] Current queue size: {}",
            queue.len()
        ));

        true
    }

    fn get_current_slot(&self) -> Slot {
        self.current_slot.get()
    }

    fn get_current_ballot(&self) -> Ballot {
        self.current_ballot.get()
    }

    fn get_proposal_by_status(&self, slot: Slot, ps: ProposalStatus) -> Option<Proposal> {
        self.proposals
            .borrow()
            .get(&slot)
            .filter(|e| e.status == ps)
            .map(|e| e.proposal.clone())
    }

    fn reserve_next_chosen_proposal(&self) -> Option<Proposal> {
        let mut map = self.proposals.borrow_mut();
        map.iter_mut()
            .find(|(_, entry)| entry.status == ProposalStatus::Chosen)
            .map(|(_slot, entry)| {
                entry.status = ProposalStatus::CommitPending;
                entry.proposal.clone()
            })
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
                logger::log_debug("[Core Proposer] No pending values available for proposal.");
                return None;
            }
        };

        let slot = self.get_next_slot();

        let prop = Proposal {
            ballot: self.get_current_ballot(),
            slot,
            value,
        };

        // Create an proposal entry with in flight status
        self.proposals.borrow_mut().insert(
            prop.slot,
            ProposalEntry {
                proposal: prop.clone(),
                status: ProposalStatus::InFlight,
            },
        );

        logger::log_info(&format!(
            "[Core Proposer] Created proposal: ballot = {}, slot = {}, value = {:?}.",
            prop.ballot, prop.slot, prop.value,
        ));

        Some(prop)
    }

    /// Processes promise responses and returns a prepare result.
    /// Chooses the highest accepted value if available.
    ///
    /// If min_slot = 0, we can use it for standalone approach.
    /// Think we need to collect all promises, and add only the no-ops to the queue for the right slot.
    /// In a standalone approach: Every accepted value should have eventually gotten to a learner.
    /// So we can start new proposals from the highest accepted slot + 1.
    ///
    /// What was here before was that we collected all accepted values and pushed them to the queue.
    /// Which meant recommit of all accepted values.
    ///
    /// If min_slot = adu, we can use it for the normal approach. Where we collect the accepted
    /// values higher than adu to get them recommitted
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

        logger::log_info(&format!(
            "[Core Proposer] Collected {} accepted values for slot: {:?}",
            accepted_values.len(),
            accepted_values
                .iter()
                .map(|p| format!("(slot: {})", p.slot))
                .collect::<Vec<_>>()
                .join(", ")
        ));

        for p in accepted_values {
            self.prioritized_values
                .borrow_mut()
                .insert(p.slot.clone(), p.value.clone());
        }

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
        let prop = match self.get_proposal_by_status(slot, ProposalStatus::InFlight) {
            Some(p) => p,
            None => {
                // This is okay it is because already accepted
                logger::log_debug(&format!(
                    "[Core Proposer] Already have quorum of accepts for slot {}",
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
            if self
                .get_proposal_by_status(slot, ProposalStatus::InFlight)
                .is_none()
            {
                return AcceptResult::MissingProposal;
            }

            AcceptResult::Accepted(count as u64)
        }
    }

    /// Marks an in‐flight proposal “chosen” once it has a quorum of accepts.
    fn mark_proposal_chosen(&self, slot: Slot) -> Option<Value> {
        if self
            .get_proposal_by_status(slot, ProposalStatus::InFlight)
            .is_none()
        {
            logger::log_error(&format!(
                "[Proposer] No in‐flight proposal for slot {} to mark as chosen.",
                slot
            ));
            return None;
        }

        let val = self.update_proposal_status(slot, ProposalStatus::Chosen)?;

        logger::log_info(&format!(
            "[Proposer] Proposal slot {} marked Chosen with value {:?}.",
            slot, val
        ));
        Some(val)
    }

    /// Marks a commit‐pending proposal “finalized” once it has executed.
    fn mark_proposal_finalized(&self, slot: Slot) -> Option<Value> {
        // swithc between infligh for acceptors send learns and commit pending for the rest
        if self
            .get_proposal_by_status(slot, ProposalStatus::InFlight)
            .is_none()
        {
            // let curr = self.proposals.borrow().get(&slot).map(|e| e.status.clone());
            // logger::log_error(&format!(
            //     "[Proposer] Cannot finalize slot {} in status {:?}; expected CommitPending.",
            //     slot, curr
            // ));
            return None;
        }

        let val = self.update_proposal_status(slot, ProposalStatus::Finalized)?;

        logger::log_info(&format!(
            "[Proposer] Proposal slot {} marked Finalized with value {:?}.",
            slot, val
        ));
        Some(val)
    }

    /// Marks and inflight proposal finalized and retires the proposal for new slot
    fn mark_proposal_finalized_and_retry(&self, slot: Slot) -> Option<Value> {
        if let Some(prop) = self.get_proposal_by_status(slot, ProposalStatus::InFlight) {
            let val = self.update_proposal_status(slot, ProposalStatus::Finalized)?;
            logger::log_info(&format!(
                "[Proposer] Proposal slot {} marked Finalized with noop: {:?} and retrying with new slot.",
                slot, &val
            ));
            self.enqueue_client_request(prop.value.clone());
            Some(prop.value)
        } else {
            // logger::log_error(&format!(
            //     "[Proposer] Cannot finalize slot {} in status {:?}; expected InFlight.",
            //     slot,
            //     self.proposals.borrow().get(&slot).map(|e| e.status.clone())
            // ));
            None
        }
    }

    fn increase_ballot(&self) -> Ballot {
        let new_ballot = self.current_ballot.get() + self.ballot_delta;
        self.current_ballot.set(new_ballot);
        logger::log_debug(&format!(
            "Core Proposer] Increased current ballot to: {}",
            new_ballot
        ));
        new_ballot
    }

    fn is_leader(&self) -> bool {
        self.is_leader.get()
    }

    // TODO: handle the process when leader change properly
    // TODO : still a todo

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
