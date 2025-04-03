#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::RefCell;
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
    Guest, GuestLearnerResource, LearnedEntry, LearnerState,
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
}

impl GuestLearnerResource for MyLearnerResource {
    /// Constructor: Initialize an empty BTreeMap.
    fn new() -> Self {
        Self {
            learned: RefCell::new(BTreeMap::new()),
        }
    }

    /// Returns the current state as a list of learned entries, sorted by slot.
    fn get_state(&self) -> LearnerState {
        let learned_list: Vec<LearnedEntry> = self
            .learned
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
    fn learn(&self, slot: Slot, value: Value) {
        let mut learned_map = self.learned.borrow_mut();
        if !learned_map.contains_key(&slot) {
            logger::log_info(&format!(
                "Learner: For slot {}, learned value {:?}",
                slot, value
            ));
            learned_map.insert(slot, value);
        } else {
            logger::log_warn(&format!(
                "Learner: Slot {} already has a learned value. Ignoring new value {:?}.",
                slot, value
            ));
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
