use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json;
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
use bindings::paxos::default::storage;

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
    node_id: String,
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
        let storage = StorageHelper::new(
            &node_id,
            500, // TODO: Get from config
            config.persistent_storage,
        );
        Self {
            config,
            num_acceptors,
            node_id: node_id,
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

        self.storage.maybe_snapshot(old_adu, new_adu, &self.learned);
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

#[derive(Deserialize, Serialize)]
struct PersistentState {
    adu: Slot,
    learned: BTreeMap<Slot, Value>,
}

struct StorageHelper {
    key: String,
    snapshot_interval: usize,
    enabled: bool,
}

impl StorageHelper {
    fn new(node_id: &String, snapshot_interval: usize, enabled: bool) -> Self {
        Self {
            key: format!("{}-learner", node_id),
            snapshot_interval,
            enabled,
        }
    }

    fn merge_snapshots(
        &self,
        snapshots: &Vec<String>,
        learned: &RefCell<BTreeMap<Slot, Value>>,
        adu: &Cell<Slot>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let mut max_adu = 0;
        let mut lm = learned.borrow_mut();
        for json in snapshots {
            let ps: PersistentState =
                serde_json::from_str(json).map_err(|e| format!("Bad snapshot JSON: {}", e))?;
            max_adu = max_adu.max(ps.adu);
            lm.extend(ps.learned);
        }
        adu.set(max_adu);
        Ok(())
    }

    fn apply_changes(
        &self,
        state_changes: &Vec<String>,
        learned: &RefCell<BTreeMap<Slot, Value>>,
        adu: &Cell<Slot>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        let mut lm = learned.borrow_mut();
        for json in state_changes {
            let entry: LearnedEntry =
                serde_json::from_str(json).map_err(|e| format!("Bad change JSON: {}", e))?;
            lm.insert(entry.slot, entry.value);
        }
        let mut next = adu.get() + 1;
        for (&slot, _) in lm.range(next..) {
            if slot == next {
                next += 1;
            } else {
                break;
            }
        }
        adu.set(next.saturating_sub(1));
        Ok(())
    }

    fn save_state_segment(&self, learned: &RefCell<BTreeMap<Slot, Value>>, adu: Slot) {
        if !self.enabled {
            return;
        }

        let now = Instant::now();
        let timestamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();

        let mut trimmed = BTreeMap::new();
        let l = learned.borrow();
        for (&slot, val) in l.iter().rev().take(self.snapshot_interval) {
            trimmed.insert(slot, val.clone());
        }

        let ps = PersistentState {
            adu,
            learned: trimmed,
        };

        match serde_json::to_string(&ps) {
            Ok(json) => {
                if let Err(e) = storage::save_state_segment(&self.key, &json, &timestamp) {
                    logger::log_error(&format!("[Core Learner] save_state_segment failed: {}", e));
                } else {
                    logger::log_warn(&format!(
                        "[Core Learner] Saved state to file in {} micros",
                        now.elapsed().as_micros()
                    ));
                }
            }
            Err(e) => {
                logger::log_error(&format!("[Core Learner] serialize snapshot failed: {}", e));
            }
        }
    }

    fn save_change(&self, learn: &LearnedEntry) {
        if !self.enabled {
            return;
        }

        let now = Instant::now();
        match serde_json::to_string(learn) {
            Ok(json) => {
                if let Err(e) = storage::save_change(&self.key, &json) {
                    logger::log_error(&format!("[Core Learner] save_change failed: {}", e));
                } else {
                    logger::log_info(&format!(
                        "[Core Learner] Saved change in {} micros",
                        now.elapsed().as_micros()
                    ));
                }
            }
            Err(e) => {
                logger::log_error(&format!("[Core Learner] serialize change failed: {}", e));
            }
        }
    }

    fn load_and_combine_state(
        &self,
        learned: &RefCell<BTreeMap<Slot, Value>>,
        adu: &Cell<Slot>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        let now = Instant::now();
        let (state_snapshots, state_changes) = storage::load_state_and_changes(&self.key)?;

        self.merge_snapshots(&state_snapshots, learned, adu)?;
        self.apply_changes(&state_changes, learned, adu)?;

        logger::log_warn(&format!(
            "[Core Learner] Loaded {} snapshots + {} changes in {}ms",
            state_snapshots.len(),
            state_changes.len(),
            now.elapsed().as_millis()
        ));
        logger::log_warn(&format!("[Core Learner] Current adu is {}", adu.get()));
        Ok(())
    }

    fn maybe_snapshot(
        &self,
        old_adu: Slot,
        new_adu: Slot,
        learned: &RefCell<BTreeMap<Slot, Value>>,
    ) {
        if !self.enabled {
            return;
        }

        let iv = self.snapshot_interval as Slot;
        if iv == 0 {
            return;
        }
        let next_boundary = ((old_adu / iv) + 1) * iv;
        if new_adu >= next_boundary {
            logger::log_info(&format!(
                "[Core Learner] ADU crossed snapshot boundary: {} → {}; persisting at {}",
                old_adu, new_adu, next_boundary
            ));
            self.save_state_segment(learned, new_adu);
        }
    }
}
