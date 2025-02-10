use log::{info, warn};
use std::cell::{Cell, RefCell};

wit_bindgen::generate!({
    world: "proposer-world",
});

use exports::paxos::wrpc::proposer::{Guest, GuestProposerResource, ProposerState};

pub struct MyProposer;

impl Guest for MyProposer {
    type ProposerResource = MyProposerResource;
}

// Cell for copy types (u64), RefCell for heap-allocated Option<String>
pub struct MyProposerResource {
    proposal_id: Cell<u64>,
    last_value: RefCell<Option<String>>,
    num_acceptors: u64,
}

impl GuestProposerResource for MyProposerResource {
    /// Constructor: Initialize the proposer with a given number of acceptors
    fn new(num_acceptors: u64) -> Self {
        Self {
            proposal_id: Cell::new(0),
            last_value: RefCell::new(None),
            num_acceptors,
        }
    }

    /// Get the current state of the proposer
    fn get_state(&self) -> ProposerState {
        ProposerState {
            proposal_id: self.proposal_id.get(),
            last_value: self.last_value.borrow().clone(),
            num_acceptors: self.num_acceptors,
        }
    }

    /// Attempt to propose a value
    fn propose(&self, proposal_id: u64, value: String) -> bool {
        let old_proposal_id = self.proposal_id.get();
        if proposal_id <= old_proposal_id {
            warn!(
                "Proposer: Rejected proposal {} (lower than current {})",
                proposal_id, old_proposal_id
            );
            return false;
        }

        self.proposal_id.set(proposal_id);
        *self.last_value.borrow_mut() = Some(value.clone());

        info!(
            "Proposer: Proposing value '{}' with ID {}",
            value, proposal_id
        );
        true
    }
}

export!(MyProposer with_types_in self);
