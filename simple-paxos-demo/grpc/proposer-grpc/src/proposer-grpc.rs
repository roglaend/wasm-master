#![allow(unsafe_op_in_unsafe_fn)]
use log::{info, warn};
use std::cell::{Cell, RefCell};

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit/paxos.wit",
        world: "proposer-world",
    });
}

bindings::export!(MyProposer with_types_in bindings);

use crate::bindings::exports::paxos::default::proposer::{
    Guest, GuestProposerResource, Proposal, ProposeResult, ProposerState,
};

struct MyProposer;

impl Guest for MyProposer {
    type ProposerResource = MyProposerResource;
}

struct MyProposerResource {
    current_ballot: Cell<u64>,
    last_proposal: RefCell<Option<Proposal>>,
    num_acceptors: u64,
    is_leader: Cell<bool>,
}

impl GuestProposerResource for MyProposerResource {
    /// Constructor: Initialize with the number of acceptors and whether it starts as leader.
    fn new(num_acceptors: u64, is_leader: bool) -> Self {
        Self {
            current_ballot: Cell::new(0),
            last_proposal: RefCell::new(None),
            num_acceptors,
            is_leader: Cell::new(is_leader),
        }
    }

    /// Return the current state of the proposer.
    fn get_state(&self) -> ProposerState {
        ProposerState {
            current_ballot: self.current_ballot.get(),
            last_proposal: self.last_proposal.borrow().clone(),
            num_acceptors: self.num_acceptors,
            is_leader: self.is_leader.get(),
        }
    }

    /// Propose a new value for a given slot.
    /// If not leader, the proposal is rejected.
    fn propose(&self, prop: Proposal) -> ProposeResult {
        if !self.is_leader.get() {
            warn!(
                "Proposer: Not leader—proposal for slot {} rejected.",
                prop.slot
            );
            return ProposeResult {
                ballot: self.current_ballot.get(),
                slot: prop.slot,
                accepted: false,
            };
        }
        // Increment the ballot for every new proposal.
        let new_ballot = self.current_ballot.get() + 1;
        self.current_ballot.set(new_ballot);
        *self.last_proposal.borrow_mut() = Some(prop.clone());

        info!(
            "Proposer: Proposing value '{}' for slot {} with new ballot {}",
            prop.value, prop.slot, new_ballot
        );
        ProposeResult {
            ballot: new_ballot,
            slot: prop.slot,
            accepted: true,
        }
    }

    /// Transition this proposer into leader mode.
    /// This bumps the ballot number and sets the leader flag to true.
    fn become_leader(&self) -> bool {
        let new_ballot = self.current_ballot.get() + 1;
        self.current_ballot.set(new_ballot);
        self.is_leader.set(true);
        info!("Proposer: Became leader with ballot {}", new_ballot);
        true
    }

    /// Relinquish leadership by marking the proposer as not leader.
    fn resign_leader(&self) -> bool {
        if self.is_leader.get() {
            self.is_leader.set(false);
            info!("Proposer: Resigned leadership.");
            true
        } else {
            warn!("Proposer: Already not leader.");
            false
        }
    }
}
