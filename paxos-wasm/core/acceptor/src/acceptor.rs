use bincode::config::Configuration;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::time::Instant;

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
    config: RunConfig,

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
        let storage = StorageHelper::new(storage_key, &config, config.persistent_storage);
        Self {
            config,
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
        self.storage.save_change(&p_value);

        // Maybe snapshot every Nth slot
        self.storage
            .maybe_snapshot(slot, &self.promises, &self.accepted);

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
}

/// Snapshot shape on disk.
#[derive(Serialize, Deserialize)]
struct PersistentState {
    promises: Vec<Ballot>,
    accepted: BTreeMap<Slot, PValue>,
}

/// Abstracts the bincode + WASI‐storage_resource logic.
struct StorageHelper {
    store: StorageResource,
    enabled: bool,
    bincode_config: Configuration,

    flush_count: usize,
    flush_interval: u64,
    snapshot_slot_interval: u64,
    snapshot_time_interval: u64,

    pending: RefCell<usize>,
    last_flush: RefCell<Instant>,
    last_snapshot_slot: Cell<Slot>,
    last_snapshot_time: RefCell<Instant>,
}

impl StorageHelper {
    fn new(key: &str, run_config: &RunConfig, enabled: bool) -> Self {
        let now = Instant::now();
        StorageHelper {
            store: StorageResource::new(key),
            enabled,
            bincode_config: bincode::config::standard(),

            flush_count: run_config.storage_flush_change_count as usize,
            flush_interval: run_config.storage_flush_change_interval_ms,
            snapshot_slot_interval: run_config.storage_snapshot_slot_interval,
            snapshot_time_interval: run_config.storage_snapshot_time_interval_ms,

            pending: RefCell::new(0),
            last_flush: RefCell::new(now),
            last_snapshot_slot: Cell::new(0),
            last_snapshot_time: RefCell::new(now),
        }
    }

    fn flush_changes(&self) {
        if !self.enabled {
            return;
        }
        self.store.flush_changes().unwrap();
        *self.pending.borrow_mut() = 0;
        *self.last_flush.borrow_mut() = Instant::now();
        logger::log_info("[Core Acceptor] flushed changelog");
    }

    /// Append one accepted‐value record.
    fn save_change(&self, pv: &PValue) {
        if !self.enabled {
            return;
        }
        let blob = bincode::serde::encode_to_vec(pv, self.bincode_config).unwrap();
        self.store.save_change(&blob).unwrap();

        let mut cnt = self.pending.borrow_mut();
        *cnt += 1;
        let now = Instant::now();
        if *cnt >= self.flush_count
            || now.duration_since(*self.last_flush.borrow()).as_millis()
                >= self.flush_interval as u128
        {
            drop(cnt);
            self.flush_changes();
        }
    }

    fn maybe_snapshot(
        &self,
        slot: Slot,
        promises: &RefCell<Vec<Ballot>>,
        accepted: &RefCell<BTreeMap<Slot, PValue>>,
    ) {
        if !self.enabled || self.snapshot_slot_interval == 0 {
            return;
        }
        let now = Instant::now();
        let last = self.last_snapshot_slot.get();
        let slot_ok = slot.saturating_sub(last) >= self.snapshot_slot_interval;
        let time_ok = now
            .duration_since(*self.last_snapshot_time.borrow())
            .as_millis()
            >= self.snapshot_time_interval as u128;
        if slot_ok || time_ok {
            // make sure all changes are on disk
            self.flush_changes();

            self.save_state_segment(promises, accepted);

            self.last_snapshot_slot.set(slot);
            *self.last_snapshot_time.borrow_mut() = now;
        }
    }

    /// Write an atomic snapshot of promises+accepted (trimmed).
    fn save_state_segment(
        &self,
        promises: &RefCell<Vec<Ballot>>,
        accepted: &RefCell<BTreeMap<Slot, PValue>>,
    ) {
        if !self.enabled || self.snapshot_slot_interval == 0 {
            return;
        }
        let now = Instant::now();

        // Trim accepted to last N entries:
        let mut trimmed = BTreeMap::new();
        for (&slot, pv) in accepted
            .borrow()
            .iter()
            .rev()
            .take(self.snapshot_slot_interval as usize)
        {
            trimmed.insert(slot, pv.clone());
        }

        let ps = PersistentState {
            promises: promises.borrow().clone(),
            accepted: trimmed,
        };
        let blob = bincode::serde::encode_to_vec(&ps, self.bincode_config).unwrap();
        let ts = Utc::now().to_rfc3339();

        self.store.save_state_segment(&blob, &ts).unwrap();
        logger::log_warn(&format!(
            "[Core Acceptor] Snapshot in {}μs",
            now.elapsed().as_micros()
        ));
    }

    /// Load and replay snapshot + changelog into the in‐memory maps.
    fn load_and_combine_state(
        &self,
        promises: &RefCell<Vec<Ballot>>,
        accepted: &RefCell<BTreeMap<Slot, PValue>>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        // Pull down whatever was persisted
        let (snapshots, changes) = self.store.load_state_and_changes()?;

        // Restore from the very last snapshot only
        let mut max_snapshot_slot = 0;
        {
            let mut p = promises.borrow_mut();
            let mut a = accepted.borrow_mut();
            if let Some(last_blob) = snapshots.last() {
                // decode it
                let (ps, _bytes_read): (PersistentState, usize) =
                    bincode::serde::decode_from_slice(last_blob, self.bincode_config)
                        .map_err(|e| e.to_string())?;

                // wipe & load
                p.clear();
                p.extend(ps.promises.clone());
                a.clear();
                a.extend(ps.accepted.clone());

                max_snapshot_slot = ps
                    .accepted
                    .keys()
                    .copied()
                    .max()
                    .unwrap_or(ps.promises.len() as Slot);
            }
        }

        // Replay any changelog entries on top
        {
            let mut a = accepted.borrow_mut();
            for blob in changes {
                let (pv, _): (PValue, usize) =
                    bincode::serde::decode_from_slice(&blob, self.bincode_config)
                        .map_err(|e| e.to_string())?;
                a.insert(pv.slot, pv);
            }
        }

        // Reset our “last snapshot” watermark so we don’t immediately re-snapshot on startup
        self.last_snapshot_slot.set(max_snapshot_slot);
        *self.last_snapshot_time.borrow_mut() = Instant::now();

        // Reset our flush counter, so we’ll batch fresh writes
        *self.pending.borrow_mut() = 0;
        *self.last_flush.borrow_mut() = Instant::now();

        logger::log_warn(&format!(
            "[Core Acceptor] Restored {} promises + {} accepted (snapshot slot={})",
            promises.borrow().len(),
            accepted.borrow().len(),
            max_snapshot_slot,
        ));

        Ok(())
    }
}
