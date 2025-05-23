use bincode::config::Configuration;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "learner-world",
        additional_derives: [
            PartialEq,
            serde::Deserialize,
            serde::Serialize,
            Clone,
            PartialOrd,
            Ord,
            Eq,
            Hash
        ],
    });
}

bindings::export!(MyLearner with_types_in bindings);

use bindings::paxos::default::learner_types::LearnResult;
use bindings::paxos::default::paxos_types::{Learn, Node, RunConfig, Slot, Value};
use bindings::paxos::default::storage::StorageResource;

use bindings::exports::paxos::default::learner::{
    Guest, GuestLearnerResource, LearnedEntry, LearnerState,
};
use bindings::paxos::default::logger;

struct MyLearner;

impl Guest for MyLearner {
    type LearnerResource = MyLearnerResource;
}

struct MyLearnerResource {
    config: RunConfig,
    num_acceptors: u64,

    slot_learns: RefCell<BTreeMap<Slot, HashMap<u64, Learn>>>, // TODO: Should be moved to learner agent to have consistent design
    learned: RefCell<BTreeMap<Slot, Value>>,
    adu: Cell<Slot>,

    storage: StorageHelper,
}

impl MyLearnerResource {
    fn quorum(&self) -> usize {
        return ((self.num_acceptors / 2) + 1) as usize;
    }

    /// Gather learns for `slot`, see if any value hits quorum.
    /// If so, returns `Some(value)`; otherwise `None`.
    fn check_quorum(&self, slot: Slot) -> Option<Value> {
        let binding = self.slot_learns.borrow();
        let slot_map = match binding.get(&slot) {
            Some(m) if m.len() >= self.quorum() => m,
            _ => return None,
        };

        // Count frequencies
        let mut counts: HashMap<&Value, usize> = HashMap::new();
        for learn in slot_map.values() {
            *counts.entry(&learn.value).or_default() += 1;
        }
        // Find any value with >= quorum
        counts
            .into_iter()
            .find(|(_, cnt)| *cnt >= self.quorum())
            .map(|(val, _)| val.clone())
    }
}

impl GuestLearnerResource for MyLearnerResource {
    fn new(num_acceptors: u64, node_id: String, config: RunConfig) -> Self {
        let storage_key = &format!("node{}-learner", node_id);
        let storage = StorageHelper::new(
            storage_key,
            500, // TODO: Get from config
            config.persistent_storage,
        );
        Self {
            config,
            num_acceptors,
            slot_learns: RefCell::new(BTreeMap::new()),
            learned: RefCell::new(BTreeMap::new()),
            adu: Cell::new(0),
            storage,
        }
    }

    fn get_state(&self) -> LearnerState {
        let list = self
            .learned
            .borrow()
            .iter()
            .map(|(&slot, v)| LearnedEntry {
                slot,
                value: v.clone(),
            })
            .collect();
        LearnerState { learned: list }
    }

    fn get_adu(&self) -> Slot {
        self.adu.get()
    }

    fn get_highest_learned(&self) -> Slot {
        self.learned
            .borrow()
            .keys()
            .last()
            .copied()
            .unwrap_or_default()
    }

    /// Try to record a learned value.  
    /// Returns `true` if we actually inserted, `false` if we already had it.
    fn learn(&self, slot: Slot, value: Value) -> bool {
        let mut learned_map = self.learned.borrow_mut();
        if learned_map.contains_key(&slot) {
            logger::log_debug(&format!(
                "[Core Learner]: Slot {} already learned, ignoring {:?}",
                slot, value
            ));
            false
        } else {
            logger::log_info(&format!(
                "[Core Learner] Recording learned value {:?} for slot {}",
                value, slot
            ));
            learned_map.insert(slot, value.clone());
            self.storage.save_change(&LearnedEntry { slot, value });
            true
        }
    }

    // Handles incoming learns from acceptors. Checks for quorum and and potentially stores the learned value.
    fn handle_learn(&self, learn: Learn, sender: Node) -> bool {
        // track the incoming Learn
        let slot = learn.slot;
        self.slot_learns
            .borrow_mut()
            .entry(slot)
            .or_default()
            .insert(sender.node_id, learn.clone());

        logger::log_info(&format!(
            "[Core Learner] Recorded vote for slot {}: current value {:?} (awaiting quorum).",
            slot, learn.value
        ));

        // now see if quorum is reached
        if let Some(quorum_value) = self.check_quorum(slot) {
            self.learn(slot, quorum_value)
        } else {
            false
        }
    }

    /// Return all newly‐executable entries in slot order, using `adu` as a cursor.
    fn to_be_executed(&self) -> LearnResult {
        let mut out = Vec::new();
        let mut next = self.adu.get() + 1;
        let old_adu = self.adu.get();
        let learned_map = self.learned.borrow();

        // Walk forward until we hit a gap
        while let Some(v) = learned_map.get(&next) {
            out.push(LearnedEntry {
                slot: next,
                value: v.clone(),
            });
            next += 1;
        }

        if out.is_empty() {
            return LearnResult::Ignore;
        }

        let new_adu = next.saturating_sub(1);
        self.adu.set(new_adu);

        let first = out.first().unwrap().slot;
        let last = out.last().unwrap().slot;
        let count = out.len();

        logger::log_info(&format!(
            "[Core Learner] executing slots {}..{} ({} entries), new adu={}",
            first, last, count, new_adu
        ));

        // If we crossed a snapshot boundary, persist:
        if (old_adu / (self.storage.snapshot_interval + 1) as u64)
            != (new_adu / (self.storage.snapshot_interval + 1) as u64)
        {
            self.storage.save_state_segment(&self.learned, new_adu);
        }
        LearnResult::Execute(out)
    }

    /// Returns the learned entry for a specific slot, if it exists.
    fn get_learned(&self, slot: u64) -> Option<LearnedEntry> {
        self.learned.borrow().get(&slot).map(|value| LearnedEntry {
            slot,
            value: value.clone(),
        })
    }

    fn load_state(&self) -> Result<(), String> {
        self.storage
            .load_and_combine_state(&self.learned, &self.adu)
    }
}

/// On‐disk snapshot structure
#[derive(Serialize, Deserialize)]
struct PersistentState {
    adu: Slot,
    learned: BTreeMap<Slot, Value>,
}

/// Wraps the WASI storage_resource and bincode serialization for the learner.
struct StorageHelper {
    store: StorageResource,
    snapshot_interval: usize,
    enabled: bool,
    bincode_config: Configuration,
}

impl StorageHelper {
    fn new(key: &str, snapshot_interval: usize, enabled: bool) -> Self {
        let store = StorageResource::new(key);
        StorageHelper {
            store,
            snapshot_interval,
            enabled,
            bincode_config: bincode::config::standard(),
        }
    }

    fn save_change(&self, entry: &LearnedEntry) {
        if !self.enabled {
            return;
        }
        let blob = bincode::serde::encode_to_vec(entry, self.bincode_config).unwrap();
        self.store.save_change(&blob).unwrap();
    }

    fn save_state_segment(&self, learned: &RefCell<BTreeMap<Slot, Value>>, adu: Slot) {
        if !self.enabled || self.snapshot_interval == 0 {
            return;
        }
        let now = Instant::now();

        // Take the last N entries
        let mut trimmed = BTreeMap::new();
        for (&slot, val) in learned.borrow().iter().rev().take(self.snapshot_interval) {
            trimmed.insert(slot, val.clone());
        }

        let ps = PersistentState {
            adu,
            learned: trimmed,
        };
        let blob = bincode::serde::encode_to_vec(&ps, self.bincode_config).unwrap();
        let ts = Utc::now().to_rfc3339();

        self.store.save_state_segment(&blob, &ts).unwrap();
        logger::log_warn(&format!(
            "[Core Learner] Snapshot in {}μs",
            now.elapsed().as_micros()
        ));
    }

    fn load_and_combine_state(
        &self,
        learned: &RefCell<BTreeMap<Slot, Value>>,
        adu: &Cell<Slot>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let (snapshots, changes) = self.store.load_state_and_changes()?;

        // Replay snapshots
        let mut max_adu = 0;
        {
            let mut lm = learned.borrow_mut();
            for blob in snapshots {
                let (ps, _): (PersistentState, usize) =
                    bincode::serde::decode_from_slice(&blob, self.bincode_config)
                        .map_err(|e| e.to_string())?;
                max_adu = max_adu.max(ps.adu);
                lm.extend(ps.learned);
            }
        }
        adu.set(max_adu);

        // Replay changelog
        {
            let mut lm = learned.borrow_mut();
            for blob in changes {
                let (entry, _): (LearnedEntry, usize) =
                    bincode::serde::decode_from_slice(&blob, self.bincode_config)
                        .map_err(|e| e.to_string())?;
                lm.insert(entry.slot, entry.value);
            }
        }

        // Advance ADU over any new contiguous slots
        let mut next = adu.get() + 1;
        for (&slot, _) in learned.borrow().range(next..) {
            if slot == next { next += 1 } else { break }
        }
        adu.set(next.saturating_sub(1));

        logger::log_warn(&format!(
            "[Core Learner] Restored adu={} with {} entries",
            adu.get(),
            learned.borrow().len()
        ));
        Ok(())
    }
}
