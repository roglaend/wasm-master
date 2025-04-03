use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "acceptor-world",
    });
}

bindings::export!(MyAcceptor with_types_in bindings);

use bindings::exports::paxos::default::acceptor::{Guest, GuestAcceptorResource};
use bindings::paxos::default::acceptor_types::{
    AcceptedResult, AcceptorState, PromiseEntry, PromiseResult,
};
use bindings::paxos::default::logger;
use bindings::paxos::default::paxos_types::{Accepted, Ballot, PValue, Promise, Slot, Value};

pub struct MyAcceptor;

impl Guest for MyAcceptor {
    type AcceptorResource = MyAcceptorResource;
}

pub struct MyAcceptorResource {
    // Map from slot to the highest promised ballot.
    promises: RefCell<BTreeMap<Slot, Ballot>>,
    // Map from slot to the accepted proposal.
    accepted: RefCell<BTreeMap<Slot, PValue>>,

    // The garbage collection window: number of recent slots to retain.
    gc_window: u64,
    // Interval of performing GC.
    gc_interval: u64,
    // The slot number when GC was last performed.
    last_gc: Cell<Slot>,
}

impl GuestAcceptorResource for MyAcceptorResource {
    fn new(gc_window: Option<u64>) -> Self {
        Self {
            promises: RefCell::new(BTreeMap::new()),
            accepted: RefCell::new(BTreeMap::new()),
            gc_window: gc_window.unwrap_or(100), // use provided gc_window or default to 100
            gc_interval: 10, // default to 10 for now, maybe pass it down from main config as with gc_window
            last_gc: Cell::new(0),
        }
    }

    /// Returns the current state: lists of promise entries and accepted proposals.
    fn get_state(&self) -> AcceptorState {
        let promises_list = self
            .promises
            .borrow()
            .iter()
            .map(|(&slot, &ballot)| PromiseEntry { slot, ballot })
            .collect();
        let accepted_list = self.accepted.borrow().values().cloned().collect();
        AcceptorState {
            promises: promises_list,
            accepted: accepted_list,
        }
    }

    /// Handles a prepare request for the given slot and ballot.
    /// Returns a promise-result:
    /// - If ballot > current promise, update and return Promised(promise).
    /// - Otherwise, return Rejected(highest_ballot).
    fn prepare(&self, slot: Slot, ballot: Ballot) -> PromiseResult {
        let mut promises = self.promises.borrow_mut();
        let highest_ballot = promises.get(&slot).cloned().unwrap_or(0);
        if ballot > highest_ballot {
            promises.insert(slot, ballot);
            logger::log_info(&format!(
                "[Core Acceptor] Slot {} updated promise to ballot {} (was {})",
                slot, ballot, highest_ballot
            ));

            // Collect all relevant accepted proposals
            let accepted = self
                .accepted
                .borrow()
                .range(slot..) // returns all key-value pairs with key >= slot
                .map(|(s, entry)| PValue {
                    slot: *s,
                    ballot: entry.ballot,
                    value: entry.value.clone(),
                })
                .collect();

            let promise = Promise {
                slot,
                ballot,
                accepted,
            };

            self.auto_garbage_collect(slot);
            PromiseResult::Promised(promise)
        } else {
            logger::log_warn(&format!(
                "[Core Acceptor] Slot {} rejected prepare with ballot {} (current highest seen ballot: {})",
                slot, ballot, highest_ballot
            ));

            PromiseResult::Rejected(highest_ballot)
        }
    }

    /// Processes an accept request for a slot.
    /// Returns AcceptedResult::Accepted if ballot matches the current promise,
    /// otherwise returns AcceptedResult::Rejected with the current highest ballot.
    fn accept(&self, slot: Slot, ballot: Ballot, value: Value) -> AcceptedResult {
        let highest_ballot = self.promises.borrow().get(&slot).cloned().unwrap_or(0);
        if ballot == highest_ballot {
            let p_value = PValue {
                slot,
                ballot,
                value: Some(value.clone()),
            };
            self.accepted.borrow_mut().insert(slot, p_value);
            logger::log_info(&format!(
                "[Core Acceptor] Accepted proposal for slot {} with ballot {}",
                slot, ballot
            ));
            self.auto_garbage_collect(slot);
            AcceptedResult::Accepted(Accepted {
                slot,
                ballot,
                success: true,
            })
        } else {
            logger::log_warn(&format!(
                "[Core Acceptor] Rejected accept for slot {} with ballot {} (current highest seen ballot: {})",
                slot, ballot, highest_ballot
            ));
            AcceptedResult::Rejected(highest_ballot)
        }
    }
}

impl MyAcceptorResource {
    fn auto_garbage_collect(&self, current_slot: Slot) {
        let next_gc_slot = self.last_gc.get() + self.gc_interval;
        if current_slot >= next_gc_slot {
            if current_slot > self.gc_window {
                let threshold = current_slot - self.gc_window;
                self.promises
                    .borrow_mut()
                    .retain(|&slot, _| slot >= threshold);
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
