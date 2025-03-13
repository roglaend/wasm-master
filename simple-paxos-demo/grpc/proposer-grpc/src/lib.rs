#![allow(unsafe_op_in_unsafe_fn)]

use bindings::exports::paxos::default::proposer::ClientProposal;
use log::{info, warn};
use std::cell::{Cell, RefCell};

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit/paxos.wit",
        world: "proposer-world",
    });
}

bindings::export!(MyProposer with_types_in bindings);

use crate::bindings::exports::paxos::default::proposer::{
    Guest, GuestProposerResource, Proposal, ProposeResult, ProposerState,
};

pub struct MyProposer;

impl Guest for MyProposer {
    type ProposerResource = MyProposerResource;
}

pub struct MyProposerResource {
    current_ballot: Cell<u64>,
    current_slot: Cell<u64>,
    last_proposal: RefCell<Option<Proposal>>,
    num_acceptors: u64,
    is_leader: Cell<bool>,
}

impl GuestProposerResource for MyProposerResource {
    /// Constructor: Initialize with the initial ballot, the number of acceptors,
    /// and whether it starts as leader.
    fn new(init_ballot: u64, num_acceptors: u64, is_leader: bool) -> Self {
        Self {
            current_ballot: Cell::new(init_ballot),
            current_slot: Cell::new(0),
            last_proposal: RefCell::new(None),
            num_acceptors,
            is_leader: Cell::new(is_leader),
        }
    }

    /// Return the current state of the proposer.
    fn get_state(&self) -> ProposerState {
        ProposerState {
            current_ballot: self.current_ballot.get(),
            current_slot: self.current_slot.get(),
            last_proposal: self.last_proposal.borrow().clone(),
            num_acceptors: self.num_acceptors,
            is_leader: self.is_leader.get(),
        }
    }

    /// Propose a new value.
    /// If not leader, the proposal is rejected.
    /// When leader, the proposer assigns the next available slot.
    fn propose(&self, client_prop: ClientProposal) -> ProposeResult {
        let proposal = Proposal {
            ballot: self.current_ballot.get(),
            slot: self.current_slot.get(),
            client_proposal: client_prop,
        };

        if !self.is_leader.get() {
            warn!("Proposer: Not leader—proposal rejected.");
            return ProposeResult {
                proposal,
                accepted: false,
            };
        }
        // Assign the next available slot internally.
        let slot = self.current_slot.get();
        self.current_slot.set(slot + 1);

        // Store the proposal value.
        *self.last_proposal.borrow_mut() = Some(proposal.clone());

        info!(
            "Proposer: Proposing value '{}' for slot {} with ballot {}",
            proposal.client_proposal.value,
            slot,
            self.current_ballot.get()
        );
        return ProposeResult {
            proposal,
            accepted: true,
        };
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
