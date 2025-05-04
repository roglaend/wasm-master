use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use bincode;
use serde::{Deserialize, Serialize, Serializer};
pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "proposer-world",
        // generate_unused_types: true,
        additional_derives: [
            PartialEq,
            serde::Deserialize,
            serde::Serialize,
            Clone,
            PartialOrd,
            Ord,
            Eq,
        ],
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
use bindings::paxos::default::storage;

pub struct MyProposer;

impl Guest for MyProposer {
    type ProposerResource = MyProposerResource;
}

#[derive(Deserialize, Serialize, Clone)]
pub struct MyProposerResource {
    #[serde(skip)]
    is_leader: Cell<bool>,
    #[serde(skip)]
    num_acceptors: u64,
    #[serde(skip)]
    ballot_delta: Ballot,
    #[serde(skip)]
    current_slot: Cell<Slot>,

    current_ballot: Cell<Ballot>,
    adu: Cell<Slot>,

    #[serde(skip)]
    node_id: String,
    #[serde(skip)]
    pending_client_requests: RefCell<VecDeque<Value>>,
    #[serde(skip)]
    prioritized_values: RefCell<VecDeque<Value>>,
    #[serde(skip)]
    proposals: RefCell<BTreeMap<Slot, ProposalEntry>>,
    // TODO: maybe persist the infliht proposals also. Although with acceptor send learns the in flight will be committed
    // even though the proposer crash. This is not the case if proposer send learns
}
impl MyProposerResource {
    fn save_state(&self) -> Result<(), String> {
        let json = serde_json::to_string(&self)
            .map_err(|e| format!("Failed to serialize to json: {}", e))?;

        storage::save_state(&self.node_id, &json)
            .map_err(|e| format!("Failed to save state: {}", e))?;

        Ok(())
    }

    fn load_state(&self) -> Result<(), String> {
        let json = storage::load_state(&self.node_id)
            .map_err(|e| format!("Failed to load state: {}", e))?;

        let state: Self = serde_json::from_str(&json)
            .map_err(|e| format!("Failed to deserialize from json: {}", e))?;

        self.current_ballot.set(state.current_ballot.get());
        self.adu.set(state.adu.get());

        logger::log_warn(&format!(
            "[Proposer] Loaded state: current_ballot = {}, adu = {}",
            state.current_ballot.get(),
            state.adu.get()
        ));

        Ok(())
    }
    fn quorum(&self) -> usize {
        return ((self.num_acceptors / 2) + 1) as usize;
    }

    fn get_next_slot(&self) -> Slot {
        let next_slot = self.current_slot.get() + 1;
        self.current_slot.set(next_slot);
        next_slot
    }

    fn next_value(&self) -> Option<Value> {
        if let Some(req) = self.prioritized_values.borrow_mut().pop_front() {
            Some(req)
        } else if let Some(req) = self.pending_client_requests.borrow_mut().pop_front() {
            Some(req)
        } else {
            None
        }
    }

    /// Collects the highest-ballot accepted value per slot across all promises,
    /// starting from `min_slot`. Fills any missing slots with a no-op proposal (None).
    fn collect_accepted_values(&self, min_slot: Slot, promises: &[Promise]) -> Vec<PValue> {
        let ballot = self.current_ballot.get();
        self.current_slot.set(min_slot - 1);

        // Map each slot to (Value for slot with highest ballot)
        let mut slot_map: HashMap<Slot, PValue> = HashMap::new();

        for promise in promises {
            for accepted in &promise.accepted {
                if accepted.slot >= min_slot {
                    slot_map
                        .entry(accepted.slot)
                        .and_modify(|existing| {
                            if accepted.ballot > existing.ballot {
                                *existing = accepted.clone();
                            }
                        })
                        .or_insert_with(|| accepted.clone());
                }
            }
        }

        // Create a sorted list of all slots that appear in the map
        let mut slots: Vec<Slot> = slot_map.keys().cloned().collect();
        slots.sort_unstable();

        let mut result = Vec::new();
        let mut expected_slot = min_slot;

        for slot in slots {
            // Fill any gaps with no-ops
            while expected_slot < slot {
                result.push(PValue {
                    slot: expected_slot,
                    ballot,
                    value: None,
                });
                expected_slot += 1;
            }

            // Add the actual accepted value
            result.push(slot_map.remove(&slot).unwrap());
            expected_slot = slot + 1;
        }

        result
    }
    fn get_proposal_by<F>(&self, slot: Slot, pred: F) -> Option<Proposal>
    where
        F: Fn(&ProposalStatus) -> bool,
    {
        self.proposals
            .borrow()
            .get(&slot)
            .filter(|e| pred(&e.status))
            .map(|e| e.proposal.clone())
    }

    fn update_proposal_status(&self, slot: Slot, new_status: ProposalStatus) -> Option<Value> {
        let mut map = self.proposals.borrow_mut();
        if let Some(entry) = map.get_mut(&slot) {
            let old_value = entry.proposal.value.clone();
            entry.status = new_status;
            Some(old_value)
        } else {
            logger::log_error(&format!(
                "[Core Proposer] Tried to update status for unknown slot {}.",
                slot
            ));
            None
        }
    }
}

// TODO: Make the proposer also increase ballot if phase 1 fails and try again

impl GuestProposerResource for MyProposerResource {
    fn new(is_leader: bool, num_acceptors: u64, init_ballot: Ballot, node_id: String) -> Self {
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
            adu: Cell::new(0),
            node_id,

            pending_client_requests: RefCell::new(VecDeque::new()),
            prioritized_values: RefCell::new(VecDeque::new()),
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

    /// Enqueues a prioritized value, called after leader change or retry.
    fn enqueue_prioritized_request(&self, req: Value) {
        self.prioritized_values.borrow_mut().push_back(req.clone());
        logger::log_debug("[Core Proposer] Enqueued prioritized request.");
    }

    fn get_current_slot(&self) -> Slot {
        self.current_slot.get()
    }

    fn get_current_ballot(&self) -> Ballot {
        self.current_ballot.get()
    }

    fn get_adu(&self) -> Slot {
        self.adu.get()
    }

    fn set_adu(&self, adu: Slot) {
        self.adu.set(adu);
        self.save_state().expect("Failed to save state");
    }

    fn get_proposal_by_status(&self, slot: Slot, ps: ProposalStatus) -> Option<Proposal> {
        self.get_proposal_by(slot, |status| *status == ps)
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
        let quorum: usize = self.quorum();
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

        let accepted_values = self.collect_accepted_values(min_slot, &promises);

        logger::log_info(&format!(
            "[Core Proposer] Collected {} accepted values: {:?}",
            accepted_values.len(),
            accepted_values
                .iter()
                .map(|p| format!("(slot: {})", p.slot))
                .collect::<Vec<_>>()
                .join(", ")
        ));

        // Deduplicate by slot and push to prioritized queue
        let mut seen_slots = HashSet::new();
        for p in accepted_values {
            if seen_slots.insert(p.slot) {
                if let Some(value) = p.value {
                    self.enqueue_prioritized_request(value);
                }
            }
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

        if count < self.quorum() {
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
                "[Core Proposer] No in‐flight proposal for slot {} to mark as chosen.",
                slot
            ));
            return None;
        }

        let val = self.update_proposal_status(slot, ProposalStatus::Chosen)?;

        logger::log_info(&format!(
            "[Core Proposer] Proposal slot {} marked Chosen with value {:?}.",
            slot, val
        ));
        Some(val)
    }

    /// Marks a proposal “finalized” once it has executed.
    fn mark_proposal_finalized(&self, slot: Slot) -> Option<Value> {
        if self
            .get_proposal_by(slot, |status| *status != ProposalStatus::Finalized)
            .is_none()
        {
            return None;
        }

        let val = self.update_proposal_status(slot, ProposalStatus::Finalized)?;
        logger::log_info(&format!(
            "[Core Proposer] Proposal slot {} marked Finalized with value {:?}.",
            slot, val
        ));
        Some(val)
    }

    fn increase_ballot(&self) -> Ballot {
        let new_ballot = self.current_ballot.get() + self.ballot_delta;
        self.current_ballot.set(new_ballot);
        logger::log_debug(&format!(
            "Core Proposer] Increased current ballot to: {}",
            new_ballot
        ));
        self.save_state().expect("Failed to save state");
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

    fn load_state(&self) -> Result<(), String> {
        self.load_state()
    }
}
