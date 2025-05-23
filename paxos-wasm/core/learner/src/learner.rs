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
        let storage = StorageHelper::new(storage_key, config, config.persistent_storage);
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

        self.storage.maybe_snapshot(new_adu, &self.learned);
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
            .load_and_replay_state(&self.learned, &self.adu)
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
    enabled: bool,
    bincode_config: Configuration,

    flush_change_count: usize,
    flush_change_interval_ms: u64,
    snapshot_slot_interval: u64,
    snapshot_time_interval_ms: u64,

    pending_changes: RefCell<usize>,
    last_flush: RefCell<Instant>,
    last_snapshot_slot: Cell<Slot>,
    last_snapshot_time: RefCell<Instant>,
}

impl StorageHelper {
    fn new(key: &str, run_config: RunConfig, enabled: bool) -> Self {
        let now = Instant::now();
        StorageHelper {
            store: StorageResource::new(key),
            enabled,
            bincode_config: bincode::config::standard(),

            flush_change_count: run_config.storage_flush_change_count as usize,
            flush_change_interval_ms: run_config.storage_flush_change_interval_ms,
            snapshot_slot_interval: run_config.storage_snapshot_slot_interval,
            snapshot_time_interval_ms: run_config.storage_snapshot_time_interval_ms,

            pending_changes: RefCell::new(0),
            last_flush: RefCell::new(now),
            last_snapshot_slot: Cell::new(0),
            last_snapshot_time: RefCell::new(now),
        }
    }

    /// actually flush the BufWriter once, amortizing many writes
    fn flush_changes(&self) {
        if !self.enabled {
            return;
        }
        self.store.flush_changes().expect("flush_changes");
        *self.pending_changes.borrow_mut() = 0;
        *self.last_flush.borrow_mut() = Instant::now();
        logger::log_info("[Core Learner] flushed changelog");
    }

    /// called on _every_ change
    fn save_change(&self, entry: &LearnedEntry) {
        if !self.enabled {
            return;
        }
        let blob = bincode::serde::encode_to_vec(entry, self.bincode_config).unwrap();
        self.store.save_change(&blob).unwrap();

        // bump our counter
        let mut cnt = self.pending_changes.borrow_mut();
        *cnt += 1;

        // maybe flush now?
        let now = Instant::now();
        if *cnt >= self.flush_change_count
            || now.duration_since(*self.last_flush.borrow()).as_millis()
                >= self.flush_change_interval_ms as u128
        {
            drop(cnt);
            self.flush_changes();
        }
    }

    /// called by learner when ADU advances
    fn maybe_snapshot(&self, new_adu: Slot, learned: &RefCell<BTreeMap<Slot, Value>>) {
        if !self.enabled {
            return;
        }

        let now = Instant::now();
        let last_slot = self.last_snapshot_slot.get();
        let slot_ok = new_adu.saturating_sub(last_slot) >= self.snapshot_slot_interval;
        let time_ok = now
            .duration_since(*self.last_snapshot_time.borrow())
            .as_millis()
            >= self.snapshot_time_interval_ms as u128;

        if slot_ok || time_ok {
            // first, push any buffered changes down
            self.flush_changes();

            self.save_state_segment(learned, new_adu);

            // record
            self.last_snapshot_slot.set(new_adu);
            *self.last_snapshot_time.borrow_mut() = now;
        }
    }

    fn save_state_segment(&self, learned: &RefCell<BTreeMap<Slot, Value>>, adu: Slot) {
        if !self.enabled || self.snapshot_slot_interval == 0 {
            return;
        }
        let now = Instant::now();

        // Take the last N entries
        let mut trimmed = BTreeMap::new();
        for (&slot, val) in learned
            .borrow()
            .iter()
            .rev()
            .take(self.snapshot_slot_interval as usize)
        {
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

    fn load_and_replay_state(
        &self,
        learned: &RefCell<BTreeMap<Slot, Value>>,
        adu: &Cell<Slot>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        // Fetch persisted bytes
        let (snapshots, changes) = self.store.load_state_and_changes()?;

        // Restore from the last snapshot only
        let mut snapshot_adu = 0;
        {
            let mut map = learned.borrow_mut();
            if let Some(last_blob) = snapshots.last() {
                let (ps, _): (PersistentState, usize) =
                    bincode::serde::decode_from_slice(last_blob, self.bincode_config)
                        .map_err(|e| e.to_string())?;
                map.clear();
                map.extend(ps.learned.clone());
                snapshot_adu = ps.adu;
                adu.set(ps.adu);
            }
        }

        // Replay every change in the changelog
        {
            let mut map = learned.borrow_mut();
            for blob in changes {
                let (entry, _): (LearnedEntry, usize) =
                    bincode::serde::decode_from_slice(&blob, self.bincode_config)
                        .map_err(|e| e.to_string())?;
                map.insert(entry.slot, entry.value);
            }
        }

        // Advance adu over any contiguous tail from the changelog
        let mut next = adu.get() + 1;
        for (&slot, _) in learned.borrow().range(next..) {
            if slot == next {
                next += 1;
            } else {
                break;
            }
        }
        adu.set(next.saturating_sub(1));

        // Reset our “last snapshot” watermark so we don’t immediately re-snapshot
        self.last_snapshot_slot.set(snapshot_adu);
        *self.last_snapshot_time.borrow_mut() = Instant::now();

        // Reset our flush counter so we’ll batch new writes cleanly
        *self.pending_changes.borrow_mut() = 0;
        *self.last_flush.borrow_mut() = Instant::now();

        logger::log_warn(&format!(
            "[Core Learner] Restored adu={} with {} entries",
            adu.get(),
            learned.borrow().len()
        ));

        Ok(())
    }
}
