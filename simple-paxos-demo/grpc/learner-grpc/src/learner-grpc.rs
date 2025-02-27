#![allow(unsafe_op_in_unsafe_fn)]

use log::info;
use std::cell::RefCell;

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit/paxos.wit",
        world: "learner-world",
    });
}

bindings::export!(MyLearner with_types_in bindings);

use crate::bindings::exports::paxos::default::learner::{
    Guest, GuestLearnerResource, LearnedEntry, LearnerState,
};

/// Local Rust representation for a learned entry.
#[derive(Clone)]
struct LearnedEntryRust {
    slot: u64,
    value: String,
}

/// Convert from our internal learned entry to the generated record.
impl From<LearnedEntryRust> for LearnedEntry {
    fn from(entry: LearnedEntryRust) -> LearnedEntry {
        LearnedEntry {
            slot: entry.slot,
            value: entry.value,
        }
    }
}

struct MyLearner;

impl Guest for MyLearner {
    type LearnerResource = MyLearnerResource;
}

/// Our learner keeps a vector of learned entries.
struct MyLearnerResource {
    learned: RefCell<Vec<LearnedEntryRust>>,
}

impl GuestLearnerResource for MyLearnerResource {
    /// Constructor: Initialize an empty learned vector.
    fn new() -> Self {
        Self {
            learned: RefCell::new(Vec::new()),
        }
    }

    /// Return the current state as a list of learned entries.
    fn get_state(&self) -> LearnerState {
        let learned_list: Vec<LearnedEntry> = self
            .learned
            .borrow()
            .iter()
            .cloned()
            .map(Into::into)
            .collect();
        LearnerState {
            learned: learned_list,
        }
    }

    /// Record that a value has been learned for a given slot.
    fn learn(&self, slot: u64, value: String) {
        info!("Learner: Learned value '{}' for slot {}", value, slot);
        self.learned
            .borrow_mut()
            .push(LearnedEntryRust { slot, value });
    }
}
