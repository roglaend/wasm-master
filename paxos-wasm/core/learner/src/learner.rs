#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "learner-world",
    });
}

bindings::export!(MyLearner with_types_in bindings);

use bindings::paxos::default::paxos_types::{Slot, Value};

use crate::bindings::exports::paxos::default::learner::{
    Guest, GuestLearnerResource, LearnResult, LearnedEntry, LearnerState,
};
use crate::bindings::paxos::default::logger;

pub struct MyLearner;

impl Guest for MyLearner {
    type LearnerResource = MyLearnerResource;
}

/// Our learner now uses a BTreeMap to store learned values per slot.
/// This ensures that each slot only has one learned value and that the entries remain ordered.
pub struct MyLearnerResource {
    learned: RefCell<BTreeMap<Slot, Value>>,
    next_to_execute: Cell<Slot>,
    execution_log: RefCell<BTreeMap<Slot, Value>>,
    // executed_order: RefCell<Vec<LearnedEntry>>,
    max_gap_size: u64,
}

impl GuestLearnerResource for MyLearnerResource {
    /// Constructor: Initialize an empty BTreeMap.
    fn new() -> Self {
        Self {
            learned: RefCell::new(BTreeMap::new()),
            next_to_execute: Cell::new(1),
            execution_log: RefCell::new(BTreeMap::new()),
            // executed_order: RefCell::new(Vec::new()),
            max_gap_size: 10,
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

    /// Record that a value has been learned for a given slot.
    /// If the slot already has a learned value, a warning is logged and the new value is ignored.
    /// Can only execute consecutive slots starting from the next_to_execute slot.
    fn learn(&self, slot: Slot, value: Value) -> LearnResult {
        let mut learned_map = self.learned.borrow_mut();
        let mut next_to_execute = self.next_to_execute.get();
        let mut execution_log = self.execution_log.borrow_mut();

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

        // Check if some learns are ready to be executed in order
        let mut to_be_executed = Vec::new();
        if learned_map.contains_key(&next_to_execute) {
            // Execute as many contiguous slots as possible.
            while let Some(val) = learned_map.remove(&next_to_execute) {
                execution_log.insert(next_to_execute, val.clone());
                to_be_executed.push(LearnedEntry {
                    slot: next_to_execute,
                    value: val.clone(),
                });

                next_to_execute += 1;
                self.next_to_execute.set(next_to_execute);
            }

            return LearnResult::Execute(to_be_executed);
        } else {
            LearnResult::Ignore
        }
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

        // Only return a gap if it exceeds the maximum allowed gap size.
        if gap > self.max_gap_size {
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
}
