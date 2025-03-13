#![allow(unsafe_op_in_unsafe_fn)]

use log::{info, warn};
use std::cell::{Cell, RefCell};

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit/paxos.wit",
        world: "acceptor-world",
    });
}

bindings::export!(MyAcceptor with_types_in bindings);

use crate::bindings::exports::paxos::default::acceptor::{
    AcceptedEntry, AcceptorState, Guest, GuestAcceptorResource,
};

/// A local Rust structure for accepted proposals.
#[derive(Clone)]
struct AcceptedEntryRust {
    slot: u64,
    ballot: u64,
    value: String,
}

/// Convert our local accepted entry to the WIT-generated `accepted_entry` record.
impl From<AcceptedEntryRust> for AcceptedEntry {
    fn from(entry: AcceptedEntryRust) -> AcceptedEntry {
        AcceptedEntry {
            slot: entry.slot,
            ballot: entry.ballot,
            value: entry.value,
        }
    }
}

pub struct MyAcceptor;

impl Guest for MyAcceptor {
    type AcceptorResource = MyAcceptorResource;
}

/// The acceptor tracks:
/// - `promised_ballot`: the highest ballot number it has promised,
/// - `accepted`: a list of proposals accepted (keyed by slot).
pub struct MyAcceptorResource {
    promised_ballot: Cell<u64>,
    accepted: RefCell<Vec<AcceptedEntryRust>>,
}

impl GuestAcceptorResource for MyAcceptorResource {
    /// Constructor: Initialize with a zero promised ballot and no accepted proposals.
    fn new() -> Self {
        Self {
            promised_ballot: Cell::new(0),
            accepted: RefCell::new(Vec::new()),
        }
    }

    /// Return the current state of the acceptor.
    fn get_state(&self) -> AcceptorState {
        let accepted_list = self
            .accepted
            .borrow()
            .iter()
            .cloned()
            .map(Into::into)
            .collect();
        AcceptorState {
            promised_ballot: self.promised_ballot.get(),
            accepted: accepted_list,
        }
    }

    /// Handle a prepare request.
    /// If the incoming ballot is higher than the current promised ballot, update it.
    fn prepare(&self, ballot: u64) -> bool {
        if ballot > self.promised_ballot.get() {
            self.promised_ballot.set(ballot);
            info!("Acceptor: Promised new ballot {}", ballot);
            true
        } else {
            warn!(
                "Acceptor: Rejected prepare for ballot {} (current promise is {})",
                ballot,
                self.promised_ballot.get()
            );
            false
        }
    }

    /// Handle an accept request.
    /// Accept the proposal only if its ballot matches the current promised ballot.
    fn accept(&self, entry: AcceptedEntry) -> bool {
        if entry.ballot == self.promised_ballot.get() {
            // Remove any previously accepted proposal for the same slot.
            self.accepted.borrow_mut().retain(|e| e.slot != entry.slot);
            self.accepted.borrow_mut().push(AcceptedEntryRust {
                slot: entry.slot,
                ballot: entry.ballot,
                value: entry.value.clone(),
            });
            info!(
                "Acceptor: Accepted proposal for slot {} with ballot {} and value '{}'",
                entry.slot, entry.ballot, entry.value
            );
            true
        } else {
            warn!(
                "Acceptor: Rejected accept for slot {} with ballot {} (current promise is {})",
                entry.slot,
                entry.ballot,
                self.promised_ballot.get()
            );
            false
        }
    }
}
