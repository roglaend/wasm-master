use bincode::config::Configuration;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

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
    _config: RunConfig,
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
        let storage = StorageHelper::new(storage_key, config.clone(), config.persistent_storage);
        Self {
            _config: config,
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
            if let Err(e) = self.storage.save_change(&LearnedEntry {
                slot,
                value: value.clone(),
            }) {
                logger::log_error(&format!(
                    "[Core Learner] failed to persist learned entry for slot {}: {}",
                    slot, e
                ));
            }
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
        drop(learned_map);

        if out.is_empty() {
            return LearnResult::Ignore;
        }

        let new_adu = next.saturating_sub(1);
        self.adu.set(new_adu);

        let first_slot = out[0].slot;
        let last_slot = out[out.len() - 1].slot;
        let count = out.len();

        logger::log_info(&format!(
            "[Core Learner] executing slots {}..{} ({} entries), new adu={}",
            first_slot, last_slot, count, new_adu
        ));

        if let Err(e) = self.storage.maybe_snapshot(new_adu, &self.learned) {
            logger::log_error(&format!(
                "[Core Learner] snapshot failed at adu={}: {}",
                new_adu, e
            ));
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
        self.storage.load_and_replay_state(&self.learned, &self.adu)
    }

    fn maybe_flush(&self) -> Result<(), String> {
        self.storage.maybe_flush_changes()
    }
}

#[derive(Serialize, Deserialize)]
struct PersistentSnapshotState {
    adu: Slot,
    learned: BTreeMap<Slot, Value>,
}

struct StorageHelper {
    store: StorageResource,
    enabled: bool,
    run_config: RunConfig,
    bincode_config: Configuration,

    // changelog
    change_flush_interval: Duration,
    change_pending: RefCell<u64>,
    change_last_flush: RefCell<Instant>,

    // snapshots
    snapshot_slot_offset: u64,
    last_snapshot_slot: Cell<Slot>,
}

impl StorageHelper {
    fn new(key: &str, run_config: RunConfig, enabled: bool) -> Self {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let offset = (hasher.finish() as u64) % run_config.storage_snapshot_slot_interval;

        let now = Instant::now();
        StorageHelper {
            store: StorageResource::new(key, run_config.clone().storage_max_snapshots),
            enabled,
            run_config: run_config.clone(),
            bincode_config: bincode::config::standard(),

            change_flush_interval: Duration::from_millis(
                run_config.clone().storage_flush_change_interval_ms,
            ),
            change_pending: RefCell::new(0),
            change_last_flush: RefCell::new(now),

            snapshot_slot_offset: offset,
            last_snapshot_slot: Cell::new(0),
        }
    }

    /// Append one learned‐entry to the changelog, flushing at your configured cadence.
    fn save_change(&self, entry: &LearnedEntry) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let blob = bincode::serde::encode_to_vec(entry, self.bincode_config)
            .map_err(|e| format!("bincode encode changes: {}", e))?;
        self.store.save_change(&blob).map_err(|e| e.to_string())?;

        *self.change_pending.borrow_mut() += 1;
        self.maybe_flush_changes()
    }

    fn maybe_flush_changes(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        let mut pending = self.change_pending.borrow_mut();
        if *pending == 0 {
            return Ok(());
        }

        let elapsed = self.change_last_flush.borrow().elapsed();
        if *pending >= self.run_config.storage_flush_change_count
            || elapsed >= self.change_flush_interval
        {
            self.flush_changes()?;
            *pending = 0;
        }
        Ok(())
    }

    fn flush_changes(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        self.store.flush_changes().map_err(|e| e.to_string())?;
        *self.change_last_flush.borrow_mut() = Instant::now();
        logger::log_info("[Core Learner] flush_changes: fsynced changelog");
        Ok(())
    }

    /// Called whenever ADU advances past `new_adu`: may snapshot+prune.
    fn maybe_snapshot(
        &self,
        new_adu: Slot,
        learned: &RefCell<BTreeMap<Slot, Value>>,
    ) -> Result<(), String> {
        if !self.enabled || self.run_config.storage_snapshot_slot_interval == 0 {
            return Ok(());
        }

        let interval = self.run_config.storage_snapshot_slot_interval;
        let offset = self.snapshot_slot_offset;
        let last = self.last_snapshot_slot.get();

        // only snapshot when new_adu ≡ offset (mod interval) AND it’s past the last one
        if new_adu % interval == offset && new_adu > last {
            self.flush_changes()?;
            self.checkpoint_snapshot(new_adu, learned)?;
            self.last_snapshot_slot.set(new_adu);
        }

        Ok(())
    }

    /// Build & write a timestamped snapshot, prune on‐disk & in‐memory.
    fn checkpoint_snapshot(
        &self,
        adu: Slot,
        learned: &RefCell<BTreeMap<Slot, Value>>,
    ) -> Result<(), String> {
        let start = Instant::now();

        // trim to the last S slots
        let trimmed: BTreeMap<_, _> = learned
            .borrow()
            .iter()
            .rev()
            .take(self.run_config.storage_snapshot_slot_interval as usize)
            .map(|(&s, v)| (s, v.clone()))
            .collect();

        // serialize + write to “snapshot-<ts>.bin”
        let ps = PersistentSnapshotState {
            adu,
            learned: trimmed.clone(),
        };
        let blob = bincode::serde::encode_to_vec(&ps, self.bincode_config)
            .map_err(|e| format!("bincode encode snapshot: {}", e))?;

        let ts = Utc::now().to_rfc3339();
        self.store.checkpoint(&blob, &ts)?;
        logger::log_info(&format!("[Core Learner] checkpoint_snapshot at {}", ts));

        // prune in‐memory to last (R×S) slots
        if self.run_config.storage_max_snapshots > 0
            && self.run_config.storage_snapshot_slot_interval > 0
        {
            let keep_span = self
                .run_config
                .storage_snapshot_slot_interval
                .saturating_mul(self.run_config.storage_max_snapshots);
            let mut map = learned.borrow_mut();
            if let Some(&max_slot) = map.keys().last() {
                let threshold = max_slot.saturating_sub(keep_span);
                map.retain(|&slot, _| slot > threshold);
                logger::log_info(&format!(
                    "[Core Learner] pruned in-memory learned to slots > {}, now {} entries",
                    threshold,
                    map.len()
                ));
            }
        }
        let elapsed_ms = start.elapsed().as_millis();
        logger::log_warn(&format!(
            "[Core Learner] checkpoint_snapshot took {} ms",
            elapsed_ms
        ));
        Ok(())
    }

    /// On startup: load last snapshots + replay changelog, restoring both `learned` and `adu`.
    fn load_and_replay_state(
        &self,
        learned: &RefCell<BTreeMap<Slot, Value>>,
        adu: &Cell<Slot>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        // Pull down up to R snapshots + all changes
        let (snapshots, changes) = self
            .store
            .load_state_and_changes(self.run_config.storage_load_snapshots)
            .map_err(|e| format!("storage.load_state_and_changes: {}", e))?;

        // Restore all loaded snapshots
        {
            let mut m = learned.borrow_mut();
            m.clear();
            for blob in &snapshots {
                let (ps, _): (PersistentSnapshotState, _) =
                    bincode::serde::decode_from_slice(blob, self.bincode_config)
                        .map_err(|e| e.to_string())?;
                // override/insert all learned entries
                for (slot, val) in ps.learned {
                    m.insert(slot, val);
                }
                // track ADU from each snapshot; at the end it will be the last one
                adu.set(ps.adu);
            }
        }

        // Replay changelog
        {
            let mut m = learned.borrow_mut();
            for blob in changes {
                let (entry, _): (LearnedEntry, _) =
                    bincode::serde::decode_from_slice(&blob, self.bincode_config)
                        .map_err(|e| e.to_string())?;
                m.insert(entry.slot, entry.value);
            }
        }

        // Advance ADU over any contiguous tail
        let mut next = adu.get() + 1;
        for &s in learned.borrow().keys().skip_while(|&&s| s <= adu.get()) {
            if s == next {
                next += 1;
            } else {
                break;
            }
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
