use log::{info, warn};
// use paxos_wrpc::proposer::acceptor;
use std::cell::{Cell, RefCell};

wit_bindgen::generate!({
    world: "proposer-world",
    // with: "paxos_wrpc:proposer/acceptor/acceptor": generate
});

use exports::paxos::wrpc::proposer::{Guest, GuestProposerResource, ProposerState};
use paxos::wrpc::acceptor::AcceptorResource;

struct MyProposer;

impl Guest for MyProposer {
    type ProposerResource = MyProposerResource;
}

// Cell for copy types (u64), RefCell for heap-allocated Option<String>
struct MyProposerResource {
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

    /// Attempt to propose a value using wRPC
    fn propose(&self, proposal_id: u64, value: String) -> bool {
        let acceptor = AcceptorResource::new();

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

        // Step 1: Send prepare request to acceptor(s)
        let prepared = acceptor.prepare(proposal_id);
        // let prepared = true;
        if !prepared {
            warn!("Proposer: Prepare phase failed");
            return false;
        }

        // Step 2: Send accept request to acceptor(s)
        let accepted = acceptor.accept(proposal_id, &value.clone());
        // let accepted = true;
        if !accepted {
            warn!("Proposer: Accept phase failed");
            return false;
        }

        info!(
            "Proposer: Value '{}' successfully proposed and accepted",
            value
        );
        true
    }
}

export!(MyProposer with_types_in self);
