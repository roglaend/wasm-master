use bincode::config::Configuration;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "acceptor-world",
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

bindings::export!(MyAcceptor with_types_in bindings);

use bindings::exports::paxos::default::acceptor::{Guest, GuestAcceptorResource};
use bindings::paxos::default::acceptor_types::{AcceptedResult, AcceptorState, PromiseResult};
use bindings::paxos::default::logger;
use bindings::paxos::default::paxos_types::{
    Accepted, Ballot, PValue, Promise, RunConfig, Slot, Value,
};
use bindings::paxos::default::storage::StorageResource;

pub struct MyAcceptor;

impl Guest for MyAcceptor {
    type AcceptorResource = MyAcceptorResource;
}

pub struct MyAcceptorResource {
    _config: RunConfig,

    promises: RefCell<Vec<Ballot>>,
    accepted: RefCell<BTreeMap<Slot, PValue>>,

    storage: StorageHelper,
}

impl MyAcceptorResource {
    /// Returns the current promised ballot (or 0 if no promise has been made yet).
    fn highest_promised_ballot(&self) -> Ballot {
        self.promises.borrow().last().cloned().unwrap_or(0)
    }

    /// Helper to collect accepted proposals for slots >= the given slot.
    fn collect_accepted_from(&self, slot: Slot) -> Vec<PValue> {
        self.accepted
            .borrow()
            .range(slot..) // returns all key-value pairs with key >= slot
            .map(|(_, p)| p.clone())
            .collect()
    }

    /// Returns the highest slot present in the accepted proposals map (or 0 if none).
    fn highest_accepted_slot(&self) -> Slot {
        self.accepted
            .borrow()
            .keys()
            .next_back()
            .cloned()
            .unwrap_or(0)
    }
}

impl GuestAcceptorResource for MyAcceptorResource {
    fn new(node_id: String, config: RunConfig) -> Self {
        let storage_key = &format!("node{}-acceptor", node_id);
        let storage = StorageHelper::new(storage_key, config.clone(), config.persistent_storage);
        Self {
            _config: config,
            promises: RefCell::new(Vec::new()),
            accepted: RefCell::new(BTreeMap::new()),
            storage,
        }
    }

    /// Processes an prepare request for a proposal.
    fn prepare(&self, slot: Slot, ballot: Ballot) -> PromiseResult {
        let highest_ballot = self.highest_promised_ballot();

        if ballot < highest_ballot {
            logger::log_warn(&format!(
                "[Core Acceptor] Slot {} rejected prepare with ballot {} (current highest promised ballot: {})",
                slot, ballot, highest_ballot
            ));
            return PromiseResult::Rejected(highest_ballot);
        }

        // Only update the promise history if the new ballot is strictly higher.
        if ballot > highest_ballot {
            self.promises.borrow_mut().push(ballot);
            logger::log_info(&format!(
                "[Core Acceptor] Added a new promise for ballot {} (was {})",
                ballot, highest_ballot
            ));

            if let Err(e) = self.storage.save_current_state(&self.promises.borrow()) {
                logger::log_error(&format!(
                    "[Core Acceptor] failed to persist promises: {}",
                    e
                ));
            }
        } else {
            logger::log_info(&format!(
                "[Core Acceptor] Received idempotent prepare for slot {} with ballot {}", // TODO: Ignore this case?
                slot, ballot
            ));
        }

        // Collect all accepted proposals (PValue) with slot >= the input slot.
        let accepted = self.collect_accepted_from(slot);

        let promise = Promise {
            slot,
            ballot,
            accepted,
        };

        PromiseResult::Promised(promise)
    }

    /// Processes an accept request for a proposal.
    fn accept(&self, slot: Slot, ballot: Ballot, value: Value) -> AcceptedResult {
        let current_promised = self.highest_promised_ballot();

        if ballot < current_promised {
            logger::log_warn(&format!(
                "[Core Acceptor] Rejected accept for slot {} with ballot {} (current highest promised ballot: {})",
                slot, ballot, current_promised
            ));
            return AcceptedResult::Rejected(current_promised);
        }

        // First check if we already have an accepted value for this slot
        let mut accepted_map = self.accepted.borrow_mut();
        if let Some(existing) = accepted_map.get(&slot) {
            // If the accepted proposal already exists:
            if existing.ballot == ballot && existing.value == Some(value.clone()) {
                // Idempotent case: the same value is being re-accepted.
                logger::log_info(&format!(
                    "[Core Acceptor] Re-accepted idempotently for slot {} with ballot {}",
                    slot, ballot
                ));
                return AcceptedResult::Accepted(Accepted {
                    slot,
                    ballot,
                    success: true,
                });
            } else if existing.value == Some(value.clone()) {
                // This could happen on leader failure where the same value is proposed again
                return AcceptedResult::Accepted(Accepted {
                    slot,
                    ballot,
                    success: true,
                });
            } else {
                // A conflicting proposal exists for this slot; reject the new request.
                logger::log_warn(&format!(
                    "[Core Acceptor] Rejected accept for slot {} with ballot {} because a conflicting proposal already exists (existing ballot: {}, value: {:?})",
                    slot, ballot, existing.ballot, existing.value
                ));
                return AcceptedResult::Rejected(existing.ballot);
            }
        }

        // No proposal has been accepted for this slot yet; accept the new proposal.
        let p_value = PValue {
            slot,
            ballot,
            value: Some(value.clone()),
        };
        accepted_map.insert(slot, p_value.clone());
        logger::log_info(&format!(
            "[Core Acceptor] Accepted proposal for slot {} with ballot {}",
            slot, ballot
        ));
        drop(accepted_map);

        // Persist the single change
        if let Err(e) = self.storage.save_change(&p_value) {
            logger::log_error(&format!("[Core Acceptor] failed to persist change: {}", e));
        }

        // Maybe snapshot every Nth slot
        if let Err(e) = self.storage.maybe_snapshot(slot, &self.accepted) {
            logger::log_error(&format!("[Core Acceptor] failed during snapshot: {}", e));
        }

        AcceptedResult::Accepted(Accepted {
            slot,
            ballot,
            success: true,
        })
    }

    fn get_accepted(&self, slot: Slot) -> Option<PValue> {
        self.accepted.borrow().get(&slot).cloned()
    }

    /// Returns the current state: lists of promise entries and accepted proposals.
    fn get_state(&self) -> AcceptorState {
        let promises_list = self.promises.borrow().clone();
        let accepted_list = self.accepted.borrow().values().cloned().collect();
        AcceptorState {
            promises: promises_list,
            accepted: accepted_list,
        }
    }

    fn load_state(&self) -> Result<(), String> {
        let res = self
            .storage
            .load_and_combine_state(&self.promises, &self.accepted);

        let highest_promised = self.highest_promised_ballot();
        let highest_accepted = self.highest_accepted_slot();
        logger::log_warn(&format!(
            "[Core Acceptor] Highest promised ballot: {}, highest accepted slot: {}",
            highest_promised, highest_accepted
        ));

        res
    }

    fn maybe_flush(&self) -> Result<(), String> {
        self.storage.maybe_flush_current_state()?;
        self.storage.maybe_flush_changes()?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct PersistentCurrentState {
    promises: Vec<Ballot>,
}

#[derive(Serialize, Deserialize)]
struct PersistentSnapshotState {
    accepted: BTreeMap<Slot, PValue>,
}

struct StorageHelper {
    store: StorageResource,
    enabled: bool,
    run_config: RunConfig,
    bincode_config: Configuration,

    // current-state
    state_flush_interval: Duration,
    state_pending: RefCell<u64>,
    state_last_flush: RefCell<Instant>,

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
            store: StorageResource::new(key, run_config.storage_max_snapshots),
            enabled,
            run_config: run_config.clone(),
            bincode_config: bincode::config::standard(),

            // promises path tuning
            state_flush_interval: Duration::from_millis(
                run_config.clone().storage_flush_state_interval_ms,
            ),
            state_pending: RefCell::new(0),
            state_last_flush: RefCell::new(now),

            // accepted path tuning
            change_flush_interval: Duration::from_millis(
                run_config.clone().storage_flush_change_interval_ms,
            ),
            change_pending: RefCell::new(0),
            change_last_flush: RefCell::new(now),

            snapshot_slot_offset: offset,
            last_snapshot_slot: Cell::new(0),
        }
    }

    /// Write out the new promises vector, buffering fsyncs.
    fn save_current_state(&self, promises: &Vec<Ballot>) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let blob = bincode::serde::encode_to_vec(
            &PersistentCurrentState {
                promises: promises.clone(),
            },
            self.bincode_config,
        )
        .map_err(|e| format!("bincode encode promises: {}", e))?;

        self.store
            .save_state(&blob)
            .map_err(|e| format!("storage.save_state promises: {}", e))?;

        *self.state_pending.borrow_mut() += 1;
        self.maybe_flush_current_state()
    }

    fn maybe_flush_current_state(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        let mut pending = self.state_pending.borrow_mut();
        if *pending == 0 {
            return Ok(());
        }

        let elapsed = self.state_last_flush.borrow().elapsed();
        if *pending >= self.run_config.storage_flush_state_count
            || elapsed >= self.state_flush_interval
        {
            self.flush_current_state()?;
            *pending = 0;
        }
        Ok(())
    }

    /// fsync the promises file (current-state.bin)
    fn flush_current_state(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        *self.state_last_flush.borrow_mut() = Instant::now();
        self.store
            .flush_state()
            .map_err(|e| format!("storage.flush_state promises: {}", e))?;
        logger::log_info("[Core Acceptor] flush_current_state: fsynced current-state");
        Ok(())
    }

    /// Read back the promises vector
    fn load_current_state(&self, promises: &RefCell<Vec<Ballot>>) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        match self.store.load_state() {
            Ok(blob) => {
                let (ps, _): (PersistentCurrentState, usize) =
                    bincode::serde::decode_from_slice(&blob, self.bincode_config)
                        .map_err(|e| format!("bincode decode promises: {}", e))?;
                let mut p = promises.borrow_mut();
                *p = ps.promises;
                logger::log_warn(&format!("[Core Acceptor] Loaded {} promises", p.len()));
                Ok(())
            }
            Err(e) if e.contains("no such file") => Ok(()),
            Err(e) => Err(format!("storage.load_state promises: {}", e)),
        }
    }

    // Save a new entry to the changelog
    fn save_change(&self, pv: &PValue) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let blob = bincode::serde::encode_to_vec(pv, self.bincode_config)
            .map_err(|e| format!("bincode encode changes: {}", e))?;
        self.store.save_change(&blob)?;

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
        self.store.flush_changes()?;
        *self.change_last_flush.borrow_mut() = Instant::now();
        logger::log_info("[Core Acceptor] flush_changes: fsynced changelog");
        Ok(())
    }

    fn maybe_snapshot(
        &self,
        slot: Slot,
        accepted: &RefCell<BTreeMap<Slot, PValue>>,
    ) -> Result<(), String> {
        if !self.enabled || self.run_config.storage_snapshot_slot_interval == 0 {
            return Ok(());
        }

        let interval = self.run_config.storage_snapshot_slot_interval;
        let offset = self.snapshot_slot_offset;
        let last = self.last_snapshot_slot.get();

        // only snapshot when slot ≡ offset (mod interval) AND we haven't already snapped this slot
        if slot % interval == offset && slot > last {
            self.flush_changes()?;
            self.checkpoint_snapshot(accepted)?;
            self.last_snapshot_slot.set(slot);
        }

        Ok(())
    }

    /// Archive a full snapshot of `accepted`, rotate on‐disk, and then prune in‐memory.
    fn checkpoint_snapshot(
        &self,
        accepted: &RefCell<BTreeMap<Slot, PValue>>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let start = Instant::now();

        // Build a trimmed snapshot of the last `snapshot_slot_interval` slots
        let trimmed: BTreeMap<_, _> = accepted
            .borrow()
            .iter()
            .rev()
            .take(self.run_config.storage_snapshot_slot_interval as usize)
            .map(|(&s, pv)| (s, pv.clone()))
            .collect();

        // Serialize and write a new on‐disk snapshot, rotating older ones
        let blob = bincode::serde::encode_to_vec(
            &PersistentSnapshotState {
                accepted: trimmed.clone(),
            },
            self.bincode_config,
        )
        .map_err(|e| format!("bincode encode snapshot: {}", e))?;

        let ts = Utc::now().to_rfc3339();
        self.store.checkpoint(&blob, &ts)?;
        logger::log_info(&format!("[Core Acceptor] checkpoint_snapshot at {}", ts));

        // Prune in‐memory `accepted` to the last R × S slots
        if self.run_config.storage_max_snapshots > 0
            && self.run_config.storage_snapshot_slot_interval > 0
        {
            let keep_span = self
                .run_config
                .storage_snapshot_slot_interval
                .saturating_mul(self.run_config.storage_max_snapshots);
            let mut map = accepted.borrow_mut();
            if let Some(&max_slot) = map.keys().last() {
                let threshold = max_slot.saturating_sub(keep_span);
                map.retain(|&slot, _| slot > threshold);
                logger::log_info(&format!(
                    "[Core Acceptor] pruned in-memory accepted to slots > {}, now {} entries",
                    threshold,
                    map.len()
                ));
            }
        }
        let elapsed_ms = start.elapsed().as_millis();
        logger::log_warn(&format!(
            "[Core Acceptor] checkpoint_snapshot took {} ms",
            elapsed_ms
        ));
        Ok(())
    }

    /// On startup or reset: load promises + accepted
    fn load_and_combine_state(
        &self,
        promises: &RefCell<Vec<Ballot>>,
        accepted: &RefCell<BTreeMap<Slot, PValue>>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        self.load_current_state(promises)?;

        let (snapshots, changes) = self
            .store
            .load_state_and_changes(self.run_config.storage_load_snapshots)
            .map_err(|e| format!("storage.load_state_and_changes: {}", e))?;

        // apply all loaded snapshots
        {
            let mut a = accepted.borrow_mut();
            a.clear();
            for blob in &snapshots {
                let (psnap, _): (PersistentSnapshotState, usize) =
                    bincode::serde::decode_from_slice(blob, self.bincode_config)
                        .map_err(|e| format!("bincode decode snapshot: {}", e))?;
                for (slot, pv) in psnap.accepted {
                    a.insert(slot, pv);
                }
            }
        }

        // replay changelog
        {
            let mut a = accepted.borrow_mut();
            for blob in changes {
                let (pv, _): (PValue, usize) =
                    bincode::serde::decode_from_slice(&blob, self.bincode_config)
                        .map_err(|e| format!("bincode decode change: {}", e))?;
                a.insert(pv.slot, pv);
            }
        }

        logger::log_warn(&format!(
            "[Core Acceptor] Restored {} promises + {} accepted",
            promises.borrow().len(),
            accepted.borrow().len()
        ));

        Ok(())
    }
}
