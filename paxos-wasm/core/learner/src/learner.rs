use chrono::Utc;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use bincode;
use serde::{Deserialize, Serialize, Serializer};

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
        ],
    });
}

bindings::export!(MyLearner with_types_in bindings);

use bindings::paxos::default::paxos_types::{Accepted, Learn, Node, Slot, Value};
use bindings::paxos::default::storage;

use crate::bindings::exports::paxos::default::learner::{
    Guest, GuestLearnerResource, LearnResult, LearnedEntry, LearnerState,
};
use crate::bindings::paxos::default::logger;

pub struct MyLearner;

impl Guest for MyLearner {
    type LearnerResource = MyLearnerResource;
}

#[derive(Deserialize, Serialize)]
struct PartialLearnerSnapshot {
    next_to_execute: Slot,
    execution_log: BTreeMap<Slot, Value>,
}

/// Our learner now uses a BTreeMap to store learned values per slot.
/// This ensures that each slot only has one learned value and that the entries remain ordered.
#[derive(Deserialize, Serialize, Clone)]
pub struct MyLearnerResource {
    next_to_execute: Cell<Slot>,
    execution_log: RefCell<BTreeMap<Slot, Value>>,

    #[serde(skip)]
    learned: RefCell<BTreeMap<Slot, Value>>,

    #[serde(skip)]
    slot_learns: RefCell<BTreeMap<Slot, HashMap<u64, Learn>>>,

    #[serde(skip)]
    node_id: String,
    #[serde(skip)]
    max_gap_size: u64,
    #[serde(skip)]
    quorum: usize,
    #[serde(skip)]
    rety_timeout: Duration,
    #[serde(skip)]
    last_message_recieved: Cell<Option<Instant>>,
}

impl MyLearnerResource {
    // fn learn_equals(&self, l1: &Learn, l2: &Learn) -> bool {
    //     l1.slot == l2.slot
    //         && l1.value.client_id == l2.value.client_id
    //         && l1.value.client_seq == l2.value.client_seq
    // }

    pub fn merge_snapshots_from_jsons(&self, snapshots: &Vec<String>) -> Result<(), String> {
        let mut max_to_execute = 0;

        for json in snapshots {
            let partial: PartialLearnerSnapshot = serde_json::from_str(&json)
                .map_err(|e| format!("Failed to parse snapshot: {}", e))?;

            max_to_execute = max_to_execute.max(partial.next_to_execute);
            // Merge execution log
            let mut execution_log = self.execution_log.borrow_mut();
            execution_log.extend(partial.execution_log);
        }
        self.next_to_execute.set(max_to_execute);
        Ok(())
    }

    pub fn apply_changes_from_json(&self, state_changes: &Vec<String>) -> Result<(), String> {
        let mut execution_log = self.execution_log.borrow_mut();

        for change_json in state_changes {
            let value: LearnedEntry = serde_json::from_str(&change_json)
                .map_err(|e| format!("Failed to parse PValue change: {}", e))?;
            execution_log.insert(value.slot, value.value);
        }

        let mut next = self.next_to_execute.get();
        while execution_log.contains_key(&next) {
            next += 1;
        }
        self.next_to_execute.set(next);

        Ok(())
    }

    fn save_state_segment(&self) -> Result<(), String> {
        let now = Instant::now();

        let timestamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();

        let execution_log = self.execution_log.borrow();
        let executions_trimmed: BTreeMap<_, _> = execution_log
            .iter()
            .rev()
            .take(500)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let snapshot = PartialLearnerSnapshot {
            next_to_execute: self.next_to_execute.get(),
            execution_log: executions_trimmed,
        };
        let json = serde_json::to_string(&snapshot).map_err(|e| e.to_string())?;

        storage::save_state_segment(&self.node_id, &json, &timestamp)?;

        let elapsed = now.elapsed();
        logger::log_warn(&format!(
            "[Core Learner] Saved state to file in {} ms",
            elapsed.as_millis()
        ));

        Ok(())
    }

    fn save_change(&self, learn: &LearnedEntry) -> Result<(), String> {
        let now = Instant::now();
        let json = serde_json::to_string(learn)
            .map_err(|e| format!("Failed to serialize to json: {}", e))?;
        storage::save_change(&self.node_id, &json)?;
        let elapsed = now.elapsed();
        logger::log_warn(&format!(
            "[Core Learner] Saved change to file in {} ms",
            elapsed.as_millis()
        ));
        Ok(())
    }

    fn load_and_combine_state(&self) -> Result<(), String> {
        let now = Instant::now();
        let (state_snapshots, state_changes) = storage::load_state_and_changes(&self.node_id)?;

        self.merge_snapshots_from_jsons(&state_snapshots)?;
        self.apply_changes_from_json(&state_changes)?;

        let elapsed = now.elapsed();
        logger::log_warn(&format!(
            "[Core Learner] Loaded state from file in {} ms",
            elapsed.as_millis()
        ));

        logger::log_warn(&format!(
            "[Core Acceptor] Loaded state from file with {} snapshots and {} changes",
            &state_snapshots.len(),
            &state_changes.len()
        ));

        let next_to_execute = self.next_to_execute.get();
        logger::log_warn(&format!(
            "[Core Acceptor] Next to execute is {}",
            &next_to_execute
        ));

        Ok(())
    }
}

impl GuestLearnerResource for MyLearnerResource {
    /// Constructor: Initialize an empty BTreeMap.
    fn new(node_id: String) -> Self {
        Self {
            learned: RefCell::new(BTreeMap::new()),
            next_to_execute: Cell::new(1),
            execution_log: RefCell::new(BTreeMap::new()),
            // executed_order: RefCell::new(Vec::new()),
            max_gap_size: 1,
            slot_learns: RefCell::new(BTreeMap::new()),
            // slots_chosen: RefCell::new(BTreeMap::new()),
            quorum: 2, // Hardcoded for now, but should be set by the config
            rety_timeout: Duration::from_millis(500),
            last_message_recieved: Cell::new(Some(Instant::now())),
            node_id,
        }
    }

    fn get_state(&self) -> LearnerState {
        let learned_list: Vec<LearnedEntry> = self
            .execution_log
            .borrow()
            .iter()
            .map(|(&slot, value)| LearnedEntry {
                slot,
                value: value.clone(),
            })
            .collect();
        LearnerState {
            learned: learned_list,
        }
    }

    fn get_next_to_execute(&self) -> Slot {
        self.next_to_execute.get()
    }

    // Handles incoming learns from acceptors. Checks for quorum and and stores the learned value. Returns ready to be executed slots if any
    fn handle_learn(&self, learn: Learn, from: Node) -> LearnResult {
        let now = Instant::now();
        // self.last_message_recieved.set(now);
        let execution_log = self.execution_log.borrow();
        if !execution_log.contains_key(&learn.slot) {
            logger::log_info(&format!(
                "[Core Learner]: Received learn for slot {} from node {}",
                learn.slot, from.node_id
            ));
            let slot = learn.slot;
            self.slot_learns
                .borrow_mut()
                .entry(slot)
                .or_insert_with(HashMap::new)
                .insert(from.node_id, learn.clone());

            if let Some(sender_map) = self.slot_learns.borrow().get(&slot) {
                if sender_map.len() >= self.quorum {
                    let learns: Vec<&Learn> = sender_map.values().collect();

                    for &candidate in &learns {
                        let count = learns.iter().filter(|&&learn| learn == candidate).count();

                        if count >= self.quorum {
                            logger::log_info(&format!(
                                "Learner: Learned full Learn {:?} for slot {} with count {}",
                                candidate, slot, count
                            ));
                            // Learn: candidate.value or the full candidate
                            self.learned
                                .borrow_mut()
                                .insert(slot, candidate.value.clone());
                            break;
                        }
                    }
                }
            }
        }
        return LearnResult::Ignore;
    }

    fn to_be_executed(&self) -> LearnResult {
        let mut learned_map = self.learned.borrow_mut();
        let mut next_to_execute = self.get_next_to_execute();

        let mut contiguous_ready = 0;
        let mut probe_slot = next_to_execute;

        // First just *count* how many contiguous slots are ready
        while learned_map.contains_key(&probe_slot) {
            contiguous_ready += 1;
            probe_slot += 1;

            if contiguous_ready >= 10 {
                // Tcp socket problems if message is to big. Also noticed increadbly slowdowns when sending 20+ slots
                break;
            }
        }

        // Ensure 10 is sent at the sime time - boost throughput by 50ops/s ca. Need to itroduce some mechanism here to ensure that
        // if we do not have 10 slots ready based on some timeout we need to send the slots we have
        // or else the system will be stuck wating for more slots to be learned.
        // This is fine when we are testing with with request_size % 10 = 0
        if contiguous_ready >= 1 {
            let mut to_be_executed = Vec::new();
            {
                let mut execution_log = self.execution_log.borrow_mut();

                // Now actually remove and execute them
                for _ in 0..contiguous_ready {
                    if let Some(val) = learned_map.remove(&next_to_execute) {
                        execution_log.insert(next_to_execute, val.clone());

                        let learned_entry = LearnedEntry {
                            slot: next_to_execute,
                            value: val,
                        };

                        to_be_executed.push(learned_entry.clone());
                        next_to_execute += 1;
                        self.save_change(&learned_entry)
                            .expect("Failed to save change");
                        self.next_to_execute.set(next_to_execute);
                    }
                }
            }

            if (self.next_to_execute.get() - 1) % 500 == 0 {
                self.save_state_segment().expect("Failed to save state");
            };

            return LearnResult::Execute(to_be_executed);
        } else {
            LearnResult::Ignore
        }
        // Check if some learns are ready to
    }
    // TODO: Take into account the client-id and client_seq when executing, and not just the slot

    /// Record that a value has been learned for a given slot.
    /// If the slot already has a learned value, a warning is logged and the new value is ignored.
    /// Can only execute consecutive slots starting from the next_to_execute slot.
    fn learn(&self, slot: Slot, value: Value) -> LearnResult {
        let mut learned_map = self.learned.borrow_mut();
        let execution_log = self.execution_log.borrow_mut();

        // Insert learn if have not learned yet
        if !learned_map.contains_key(&slot) && !execution_log.contains_key(&slot) {
            logger::log_info(&format!(
                "[Core Learner]: For slot {}, learned value {:?}",
                slot, value
            ));
            learned_map.insert(slot, value);
        } else {
            logger::log_warn(&format!(
                "Learner: Slot {} already has a learned value. Ignoring new value {:?}.",
                slot, value
            ));
        }
        return LearnResult::Ignore;
    }

    // Checker for gaps in the learned slots. Should be called at reasoinable a interval.
    fn check_for_gap(&self) -> Option<Slot> {
        let learned_map = self.learned.borrow_mut();
        let next_to_execute = self.next_to_execute.get();
        let max_learned_slot = learned_map.keys().max().copied().unwrap_or(0);

        // If no slot beyond next_to_execute has been learned, nothing to gap.
        if max_learned_slot <= next_to_execute {
            return None;
        }

        // If the next slot is already learned, there is no gap.
        if learned_map.contains_key(&next_to_execute) {
            return None;
        }

        // Compute the gap between the maximum learned slot and the next expected one.
        let gap = max_learned_slot - next_to_execute;

        let now = Instant::now();
        // return if gap is > max_gap_size or if the time since last message is > retry_timeout
        if gap >= self.max_gap_size
            || (gap > 0
                && now.duration_since(self.last_message_recieved.get()?) > self.rety_timeout)
        {
            Some(next_to_execute)
        } else {
            None
        }
    }

    /// Returns the learned entry for a specific slot, if it exists.
    fn get_learned(&self, slot: u64) -> Option<LearnedEntry> {
        self.learned.borrow().get(&slot).map(|value| LearnedEntry {
            slot,
            value: value.clone(),
        })
    }

    fn load_state(&self) -> Result<(), String> {
        self.load_and_combine_state()
    }
}
