use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use bincode::config::Configuration;
use serde::{Deserialize, Serialize};
mod bindings {
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
    Accepted, Ballot, PValue, Promise, Proposal, RunConfig, Slot, Value,
};
use bindings::paxos::default::proposer_types::{
    AcceptResult, PrepareResult, ProposalEntry, ProposalStatus,
};
use bindings::paxos::default::storage::StorageResource;

struct MyProposer;

impl Guest for MyProposer {
    type ProposerResource = MyProposerResource;
}

struct MyProposerResource {
    num_acceptors: u64,
    ballot_delta: Ballot,

    is_leader: Cell<bool>,
    current_slot: Cell<Slot>,
    current_ballot: Cell<Ballot>,
    adu: Cell<Slot>,

    pending_client_requests: RefCell<VecDeque<Value>>,
    prioritized_values: RefCell<VecDeque<Value>>,
    proposals: RefCell<BTreeMap<Slot, ProposalEntry>>,
    chosen_slots: RefCell<VecDeque<Slot>>,

    // TODO: maybe persist the inflight proposals also. Although with acceptor send learns the in flight will be committed
    // TODO: even though the proposer crash. This is not the case if proposer send learns
    storage: StorageHelper,
}
impl MyProposerResource {
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
        // If last prepare came in too late and we had already moved on, this was setting the slot back to prepare slot
        if self.current_slot.get() > min_slot {
            logger::log_warn(&format!(
                "[Core Proposer] Current slot {} is greater than min_slot {}. No accepted values will be collected.",
                self.current_slot.get(),
                min_slot
            ));
            return Vec::new();
        }

        let ballot = self.current_ballot.get();
        self.current_slot.set(min_slot.saturating_sub(1));

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

impl GuestProposerResource for MyProposerResource {
    fn new(
        is_leader: bool,
        num_acceptors: u64,
        init_ballot: Ballot,
        node_id: String,
        config: RunConfig,
    ) -> Self {
        let storage_key = &format!("node{}-proposer", node_id);
        let storage = StorageHelper::new(&storage_key, config.clone(), config.persistent_storage);

        logger::log_info(&format!(
            "[Core Proposer] Initialized as {} node with {} acceptors and initial ballot {}.",
            if is_leader { "leader" } else { "normal" },
            num_acceptors,
            init_ballot
        ));

        Self {
            num_acceptors,
            ballot_delta: num_acceptors, // TODO: This right?

            is_leader: Cell::new(is_leader),
            current_slot: Cell::new(0),
            current_ballot: Cell::new(init_ballot),
            adu: Cell::new(0),

            pending_client_requests: RefCell::new(VecDeque::new()),
            prioritized_values: RefCell::new(VecDeque::new()),
            proposals: RefCell::new(BTreeMap::new()),
            chosen_slots: RefCell::new(VecDeque::new()),

            storage,
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
        let mut prioritized_values = self.prioritized_values.borrow_mut();
        if prioritized_values.contains(&req) {
            return;
        }
        prioritized_values.push_back(req.clone());
        logger::log_info(&format!(
            "[Core Proposer] Enqueued prioritized request with Value: {:?}",
            req
        ));
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
        self.storage
            .save_state(self.current_ballot.get(), adu)
            .expect("Failed to save state");
    }

    fn get_proposal_by_status(&self, slot: Slot, ps: ProposalStatus) -> Option<Proposal> {
        self.get_proposal_by(slot, |status| *status == ps)
    }

    fn reserve_next_chosen_proposal(&self) -> Option<Proposal> {
        let slot = self.chosen_slots.borrow_mut().pop_front()?;

        if let Some(entry) = self.proposals.borrow_mut().get_mut(&slot) {
            entry.status = ProposalStatus::CommitPending;
            return Some(entry.proposal.clone());
        }
        None
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
            "[Core Proposer] Collected {} previously accepted values",
            accepted_values.len(),
        ));

        // Deduplicate by slot and push to prioritized queue
        let mut seen_slots = HashSet::new();
        accepted_values
            .into_iter()
            .filter(|p| seen_slots.insert(p.slot))
            .filter_map(|p| p.value)
            .filter(|v| v.command.is_some())
            .for_each(|v| self.enqueue_prioritized_request(v));

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
        self.chosen_slots.borrow_mut().push_back(slot);

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
        self.storage
            .save_state(new_ballot, self.adu.get())
            .expect("Failed to save state");
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
        self.storage.load_state(&self.current_ballot, &self.adu)
    }

    fn maybe_flush(&self) -> Result<(), String> {
        self.storage.maybe_flush_state()
    }
}

#[derive(Deserialize, Serialize)]
struct PersistentCurrentState {
    current_ballot: Ballot,
    adu: Slot,
}

struct StorageHelper {
    store: StorageResource,
    enabled: bool,
    run_config: RunConfig,
    bincode_config: Configuration,

    flush_interval: Duration,
    pending: RefCell<u64>,
    last_flush: RefCell<Instant>,
}

impl StorageHelper {
    fn new(key: &str, run_config: RunConfig, enabled: bool) -> Self {
        let now = Instant::now();
        StorageHelper {
            store: StorageResource::new(key, run_config.storage_max_snapshots),
            enabled,
            run_config: run_config.clone(),
            bincode_config: bincode::config::standard(),

            flush_interval: Duration::from_millis(
                run_config.clone().storage_flush_state_interval_ms,
            ),
            pending: RefCell::new(0),
            last_flush: RefCell::new(now),
        }
    }

    /// Called by the proposer every time we want to overwrite current-state.
    /// Buffers up to `flush_state_count` calls or `flush_state_interval` duration.
    fn save_state(&self, current_ballot: Ballot, adu: Slot) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        let ps = PersistentCurrentState {
            current_ballot,
            adu,
        };
        let blob = bincode::serde::encode_to_vec(&ps, self.bincode_config)
            .map_err(|e| format!("bincode encode: {}", e))?;
        self.store
            .save_state(&blob)
            .map_err(|e| format!("storage.save_state: {}", e))?;

        *self.pending.borrow_mut() += 1;
        self.maybe_flush_state()
    }

    fn maybe_flush_state(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        let mut pending = self.pending.borrow_mut();
        if *pending == 0 {
            return Ok(());
        }

        let elapsed = self.last_flush.borrow().elapsed();
        if *pending >= self.run_config.storage_flush_state_count || elapsed >= self.flush_interval {
            self.flush_state()?;
            *pending = 0;
        }
        Ok(())
    }

    /// Performs the real fsync of current-state.bin
    fn flush_state(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        *self.last_flush.borrow_mut() = Instant::now();
        self.store
            .flush_state()
            .map_err(|e| format!("storage.flush_state: {}", e))?;
        logger::log_info("[Core Learner] flush_state: fsynced current-state");
        Ok(())
    }

    /// On startup: load & decode exactly once
    fn load_state(&self, ballot: &Cell<Ballot>, adu: &Cell<Slot>) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let blob = match self.store.load_state() {
            Ok(data) => data,
            Err(e) if e.contains("no such file") => return Ok(()),
            Err(e) => return Err(format!("storage.load_state: {}", e)),
        };
        let (ps, _): (PersistentCurrentState, usize) =
            bincode::serde::decode_from_slice(&blob, self.bincode_config)
                .map_err(|e| format!("bincode decode: {}", e))?;

        ballot.set(ps.current_ballot);
        adu.set(ps.adu);
        logger::log_warn(&format!(
            "[Core Proposer] Loaded state: ballot={} adu={}",
            ps.current_ballot, ps.adu
        ));
        Ok(())
    }
}
