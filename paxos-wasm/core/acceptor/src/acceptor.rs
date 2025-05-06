use chrono::Utc;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use std::fs;
use std::io::Write;
use std::time::Instant;

use bincode;
use serde::{Deserialize, Serialize, Serializer};

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
use bindings::paxos::default::paxos_types::{Accepted, Ballot, PValue, Promise, Slot, Value};
use bindings::paxos::default::storage;

pub struct MyAcceptor;

impl Guest for MyAcceptor {
    type AcceptorResource = MyAcceptorResource;
}

#[derive(Deserialize, Serialize)]
struct PartialAcceptorSnapshot {
    promises: Vec<Ballot>,
    accepted: BTreeMap<Slot, PValue>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct MyAcceptorResource {
    promises: RefCell<Vec<Ballot>>,
    accepted: RefCell<BTreeMap<Slot, PValue>>,

    #[serde(skip)]
    node_id: String,
    #[serde(skip)]
    gc_window: u64,
    #[serde(skip)]
    gc_interval: u64,
    #[serde(skip)]
    last_gc: Cell<Slot>,
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

    pub fn merge_snapshots_from_jsons(&self, snapshots: &Vec<String>) -> Result<(), String> {
        for json in snapshots {
            let partial: PartialAcceptorSnapshot = serde_json::from_str(&json)
                .map_err(|e| format!("Failed to parse snapshot: {}", e))?;

            // Merge promises
            let mut promises = self.promises.borrow_mut();
            promises.extend(partial.promises);

            // Merge accepted
            let mut accepted = self.accepted.borrow_mut();
            accepted.extend(partial.accepted);
        }
        Ok(())
    }

    pub fn apply_changes_from_json(&self, state_changes: &Vec<String>) -> Result<(), String> {
        let mut accepted = self.accepted.borrow_mut();

        for change_json in state_changes {
            let pvalue: PValue = serde_json::from_str(&change_json)
                .map_err(|e| format!("Failed to parse PValue change: {}", e))?;
            accepted.insert(pvalue.slot, pvalue);
        }

        Ok(())
    }

    fn save_state_segment(&self) -> Result<(), String> {
        let now = Instant::now();

        // let bytes = bincode::serialize(&self);
        let timestamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();

        let accepted = self.accepted.borrow();
        let acceptetd_trimmed: BTreeMap<_, _> = accepted
            .iter()
            .rev()
            .take(500)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let snapshot = PartialAcceptorSnapshot {
            promises: self.promises.borrow().clone(),
            accepted: acceptetd_trimmed,
        };

        let json = serde_json::to_string(&snapshot)
            .map_err(|e| format!("Failed to serialize to json: {}", e))?;

        // Only take last 500 slots in accepted map

        let key = format!("{}-{}", self.node_id, "acceptor");

        storage::save_state_segment(&key, &json, &timestamp)?;

        let elapsed = now.elapsed();
        logger::log_warn(&format!(
            "[Core Acceptor] Saved state to file in {} ms",
            elapsed.as_millis()
        ));

        Ok(())
    }

    fn save_change(&self, accepted: &PValue) -> Result<(), String> {
        let now = Instant::now();
        let json = serde_json::to_string(accepted)
            .map_err(|e| format!("Failed to serialize to json: {}", e))?;

        let key = format!("{}-{}", self.node_id, "acceptor");
        storage::save_change(&key, &json)?;
        let elapsed = now.elapsed();
        logger::log_info(&format!(
            "[Core Acceptor] Saved change to file in {} ms",
            elapsed.as_millis()
        ));
        Ok(())
    }

    fn load_and_combine_state(&self) -> Result<(), String> {
        let now = Instant::now();
        let key = format!("{}-{}", self.node_id, "acceptor");
        let (state_snapshots, state_changes) = storage::load_state_and_changes(&key)?;
        self.merge_snapshots_from_jsons(&state_snapshots)?;
        self.apply_changes_from_json(&state_changes)?;

        let elapsed = now.elapsed();
        logger::log_warn(&format!(
            "[Core Acceptor] Loaded state from file in {} ms",
            elapsed.as_millis()
        ));

        logger::log_warn(&format!(
            "[Core Acceptor] Loaded state from file with {} snapshots and {} changes",
            &state_snapshots.len(),
            &state_changes.len()
        ));

        let highest_promised = self.highest_promised_ballot();
        let highest_accepted = self.highest_accepted_slot();
        logger::log_warn(&format!(
            "[Core Acceptor] Highest promised ballot: {}, highest accepted slot: {}",
            highest_promised, highest_accepted
        ));

        Ok(())
    }
}

impl GuestAcceptorResource for MyAcceptorResource {
    fn new(gc_window: Option<u64>, node_id: String) -> Self {
        Self {
            promises: RefCell::new(Vec::new()),
            accepted: RefCell::new(BTreeMap::new()),
            node_id,
            gc_window: gc_window.unwrap_or(100), // use provided gc_window or default to 100
            gc_interval: 10, // default to 10 for now, maybe pass it down from main config as with gc_window
            last_gc: Cell::new(0),
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
        // logger::log_info(&format!(
        //     "[Core Acceptor] Collected {} accepted proposals for slot: {:?}",
        //     accepted.len(),
        //     accepted
        //         .iter()
        //         .map(|p| format!("(slot: {})", p.slot))
        //         .collect::<Vec<_>>()
        //         .join(", ")
        // ));

        let promise = Promise {
            slot,
            ballot,
            accepted,
        };

        // self.storage_test_json(); // TODO: Remove this line after testing.

        // self.auto_garbage_collect(slot);
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
            // self.auto_garbage_collect(slot);
            // self.storage_test_json();

            // save change every request
            self.save_change(&p_value).expect("Failed to save change");

            // save the state every 500 slots
            if slot % 500 == 0 {
                self.save_state_segment().expect("Failed to save state");
            };

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
        self.load_and_combine_state()
    }
}

impl MyAcceptorResource {
    fn auto_garbage_collect(&self, current_slot: Slot) {
        let next_gc_slot = self.last_gc.get() + self.gc_interval;
        if current_slot >= next_gc_slot {
            if current_slot > self.gc_window {
                let threshold = current_slot - self.gc_window;
                // self.promises
                //     .borrow_mut()
                //     .retain(|&slot, _| slot >= threshold); // TODO: Enable if too many promises causes and issue.
                self.accepted
                    .borrow_mut()
                    .retain(|&slot, _| slot >= threshold);
                logger::log_info(&format!(
                    "[Core Acceptor] Auto garbage collected state for slots below {}",
                    threshold
                ));
            }
            // Update the last GC slot to the current slot.
            self.last_gc.set(current_slot);
        }
    }
}
