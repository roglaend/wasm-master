use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
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
use bindings::paxos::default::storage;

pub struct MyAcceptor;

impl Guest for MyAcceptor {
    type AcceptorResource = MyAcceptorResource;
}

pub struct MyAcceptorResource {
    config: RunConfig,
    node_id: String,

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
        let storage = StorageHelper::new(
            &node_id,
            500, // TODO: Get from config
            config.persistent_storage,
        );
        Self {
            config,
            node_id,
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

        let mut accepted_map = self.accepted.borrow_mut();
        if let Some(existing) = accepted_map.get(&slot) {
            // If the accepted proposal already exists:
            if existing.ballot == ballot && existing.value == Some(value.clone()) {
                // Idempotent case: the same value is being re-accepted.  // TODO: Might not be needed, could maybe just ignore instead.
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
                // this could happen on leader failure where leader proposes values from promises to get them executed
                // the leader could probably directly commit such a value but then we cant use them in the
                // priority queue as we have now
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
        } else {
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

            self.storage
                .save_change(&p_value)
                .expect("Failed to save change");
            self.storage
                .maybe_snapshot(slot, &self.promises, &self.accepted);

            return AcceptedResult::Accepted(Accepted {
                slot,
                ballot,
                success: true,
            });
        }
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

/// The on‐disk shape of a snapshot.
#[derive(Deserialize, Serialize)]
struct PersistentState {
    promises: Vec<Ballot>,
    accepted: BTreeMap<Slot, PValue>,
}

/// Encapsulates *all* of the serde/json + storage calls for the Acceptor.
struct StorageHelper {
    key: String,
    snapshot_interval: usize,
    enabled: bool,
}

impl StorageHelper {
    fn new(node_id: &str, snapshot_interval: usize, enabled: bool) -> Self {
        Self {
            key: format!("{}-acceptor", node_id),
            snapshot_interval,
            enabled,
        }
    }

    fn merge_snapshots(
        &self,
        snaps: &[String],
        promises: &RefCell<Vec<Ballot>>,
        accepted: &RefCell<BTreeMap<Slot, PValue>>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let mut p = promises.borrow_mut();
        let mut a = accepted.borrow_mut();
        for json in snaps {
            let partial: PersistentState =
                serde_json::from_str(json).map_err(|e| format!("Bad snapshot JSON: {}", e))?;
            p.extend(partial.promises);
            a.extend(partial.accepted);
        }
        Ok(())
    }

    fn apply_changes(
        &self,
        changes: &[String],
        accepted: &RefCell<BTreeMap<Slot, PValue>>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let mut a = accepted.borrow_mut();
        for json in changes {
            let p_value: PValue =
                serde_json::from_str(json).map_err(|e| format!("Bad change JSON: {}", e))?;
            a.insert(p_value.slot, p_value);
        }
        Ok(())
    }

    fn save_state_segment(
        &self,
        promises: &RefCell<Vec<Ballot>>,
        accepted: &RefCell<BTreeMap<Slot, PValue>>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let now = Instant::now();
        let timestamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();

        // Trim to last `snapshot_interval` entries
        let mut a_trim = BTreeMap::new();
        for (&slot, v) in accepted.borrow().iter().rev().take(self.snapshot_interval) {
            a_trim.insert(slot, v.clone());
        }

        let snapshot = PersistentState {
            promises: promises.borrow().clone(),
            accepted: a_trim,
        };
        let json =
            serde_json::to_string(&snapshot).map_err(|e| format!("serialize snapshot: {}", e))?;

        storage::save_state_segment(&self.key, &json, &timestamp)?;

        logger::log_warn(&format!(
            "[Core Acceptor] Saved snapshot in {} micros",
            now.elapsed().as_micros()
        ));
        Ok(())
    }

    fn save_change(&self, p_value: &PValue) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let now = Instant::now();
        let json =
            serde_json::to_string(p_value).map_err(|e| format!("serialize change: {}", e))?;
        storage::save_change(&self.key, &json)?;
        logger::log_info(&format!(
            "[Core Acceptor] Saved change in {} micros",
            now.elapsed().as_micros()
        ));
        Ok(())
    }

    fn load_and_combine_state(
        &self,
        promises: &RefCell<Vec<Ballot>>,
        accepted: &RefCell<BTreeMap<Slot, PValue>>,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let now = Instant::now();
        let (snaps, changes) = storage::load_state_and_changes(&self.key)?;
        self.merge_snapshots(&snaps, promises, accepted)?;
        self.apply_changes(&changes, accepted)?;
        logger::log_warn(&format!(
            "[Core Acceptor] Loaded {} snapshots + {} changes in {}ms",
            snaps.len(),
            changes.len(),
            now.elapsed().as_millis()
        ));
        Ok(())
    }

    /// If we’ve crossed another snapshot boundary, persist one now.
    fn maybe_snapshot(
        &self,
        slot: Slot,
        promises: &RefCell<Vec<Ballot>>,
        accepted: &RefCell<BTreeMap<Slot, PValue>>,
    ) {
        if !self.enabled {
            return;
        }
        let iv = self.snapshot_interval as Slot;
        if iv == 0 {
            return;
        }
        // e.g. boundary at 500, 1000, 1500…
        if slot % iv == 0 {
            // we chose a simple “every Nth slot” rule here
            if let Err(e) = self.save_state_segment(promises, accepted) {
                logger::log_error(&format!("Snapshot failed: {}", e));
            }
        }
    }
}
