#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "acceptor-world",
    });
}

bindings::export!(MyAcceptor with_types_in bindings);

use bindings::exports::paxos::default::acceptor::{Guest, GuestAcceptorResource};
use bindings::paxos::default::acceptor_types::{AcceptedEntry, AcceptorState, PromiseEntry};
use bindings::paxos::default::logger;
use bindings::paxos::default::paxos_types::{Ballot, Slot, Prepare, Promise, Accept, Learn, Accepted};

pub struct MyAcceptor;

impl Guest for MyAcceptor {
    type AcceptorResource = MyAcceptorResource;
}

/// The acceptor resource now maintains per-slot promises and accepted proposals.
pub struct MyAcceptorResource {
    // Map from slot to the highest promised ballot for that slot.
    promises: RefCell<HashMap<Slot, Ballot>>,
    // Map from slot to the accepted proposal (if any).
    accepted: RefCell<HashMap<u64, AcceptedEntry>>,


    rnd: Cell<u64>, //highest round the acceptor has promised in
    highest_seen: Cell<u64>,
    accepted2: RefCell<HashMap<u64, AcceptedEntry>>,
}

impl GuestAcceptorResource for MyAcceptorResource {
    /// Constructor: Initialize with empty promise and accepted maps.
    fn new() -> Self {
        Self {
            promises: RefCell::new(HashMap::new()),
            accepted: RefCell::new(HashMap::new()),

            rnd: Cell::new(0),
            highest_seen: Cell::new(0),
            
            accepted2: RefCell::new(HashMap::new())
        }
    }

    /// Returns the current state of the acceptor.
    /// This includes a list of promise entries (one per slot) and the accepted proposals.
    fn get_state(&self) -> AcceptorState {
        // Build a list of promise entries from the promises map.
        let promises_list: Vec<PromiseEntry> = self
            .promises
            .borrow()
            .iter()
            .map(|(&slot, &ballot)| PromiseEntry { slot, ballot })
            .collect();

        // Build a list of accepted proposals from the accepted map.
        let accepted_list: Vec<AcceptedEntry> = self
            .accepted
            .borrow()
            .values()
            .cloned()
            .map(Into::into)
            .collect();

        AcceptorState {
            promises: promises_list,
            accepted: accepted_list,
        }
    }

    /// Handle a prepare request for a given slot and ballot.
    ///
    /// The acceptor will promise not to accept proposals with a lower ballot for this slot.
    /// Returns true if the incoming ballot is higher than the current promise for the slot.
    fn prepare(&self, slot: Slot, ballot: Ballot) -> bool {
        let mut promises = self.promises.borrow_mut();
        let current_promise = promises.get(&slot).cloned().unwrap_or(0);
        if ballot > current_promise {
            promises.insert(slot, ballot);
            logger::log_info(&format!(
                "Acceptor: For slot {}, updated promise to ballot {} (was {})",
                slot, ballot, current_promise
            ));
            true
        } else {
            logger::log_warn(&format!(
                "Acceptor: For slot {}, rejected prepare with ballot {} (current promise is {})",
                slot, ballot, current_promise
            ));
            false
        }
    }

    /// Handle an accept request.
    ///
    /// A proposal is accepted only if its ballot matches the promised ballot for that slot.
    /// When accepted, the proposal is recorded (overwriting any previous proposal for the slot).
    fn accept(&self, entry: AcceptedEntry) -> bool {
        let slot = entry.slot;
        let promised_ballot = self.promises.borrow().get(&slot).cloned().unwrap_or(0);
        if entry.ballot == promised_ballot {
            // Accept the proposal by storing it for this slot.
            self.accepted.borrow_mut().insert(
                slot,
                AcceptedEntry {
                    slot,
                    ballot: entry.ballot,
                    value: entry.value.clone(),
                },
            );
            logger::log_info(&format!(
                "Acceptor: Accepted proposal for slot {} with ballot {} and value {:?}",
                slot, entry.ballot, entry.value
            ));
            true
        } else {
            logger::log_warn(&format!(
                "Acceptor: For slot {}, rejected accept request with ballot {} (expected ballot {})",
                slot, entry.ballot, promised_ballot
            ));
            false
        }
    }

    fn handle_prepare(&self, prepare: Prepare) -> Option<Promise> {
        if prepare.slot > self.highest_seen.get() {
            self.highest_seen.set(prepare.slot);
        }

        if prepare.ballot < self.rnd.get() {
            return None
        }

        self.rnd.set(prepare.ballot);

        let promise = Promise { 
            ballot: self.rnd.get(),
            accepted: Vec::new()
        };

        // include accepted values greater than prepare slot (means values that were accepted but never committed)
        Some(promise)

        // fire_and fo

    }

    fn handle_accept(&self, accept: Accept) -> Option<Accepted> {
        if accept.slot < self.highest_seen.get() && accept.ballot < self.rnd.get() {
            return  None;
        }

        self.highest_seen.set(accept.slot);

        if accept.ballot < self.rnd.get() {
            return  None;
        }

        let accepted_value = AcceptedEntry {
            slot: accept.slot,
            ballot: accept.ballot,
            value: accept.value.clone()
        };

        self.accepted2.borrow_mut().insert(accept.slot, accepted_value);

        let accepted = Accepted {
            slot: accept.slot,
            ballot: accept.ballot,
            success: true,
            value: accept.value
        };
        Some(accepted)

    }
}
