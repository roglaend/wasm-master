use log::{info, warn};
use std::cell::{Cell, RefCell};

wit_bindgen::generate!({
    world: "acceptor-world",
});

use exports::paxos::wrpc::acceptor::{AcceptorState, Guest, GuestAcceptorResource};

struct MyAcceptor;

impl Guest for MyAcceptor {
    type AcceptorResource = MyAcceptorResource;
}

// Cell for copy types (u64), RefCell for heap-allocated Option<String>
struct MyAcceptorResource {
    promised_id: Cell<u64>,
    accepted_id: RefCell<Option<u64>>,
    accepted_value: RefCell<Option<String>>,
}

impl GuestAcceptorResource for MyAcceptorResource {
    /// Constructor: Initialize the acceptor
    fn new() -> Self {
        Self {
            promised_id: Cell::new(0),
            accepted_id: RefCell::new(None),
            accepted_value: RefCell::new(None),
        }
    }

    /// Get the current state of the acceptor
    fn get_state(&self) -> AcceptorState {
        AcceptorState {
            promised_id: self.promised_id.get(),
            accepted_id: *self.accepted_id.borrow(),
            accepted_value: self.accepted_value.borrow().clone(),
        }
    }

    /// Handle a prepare request (Phase 1A)
    fn prepare(&self, proposal_id: u64) -> bool {
        if proposal_id > self.promised_id.get() {
            self.promised_id.set(proposal_id);
            info!("Acceptor: Promised proposal {}", proposal_id);
            return true;
        }
        warn!("Acceptor: Rejected prepare request for {}", proposal_id);
        false
    }

    /// Handle an accept request (Phase 2A)
    fn accept(&self, proposal_id: u64, value: String) -> bool {
        if proposal_id >= self.promised_id.get() {
            self.accepted_id.replace(Some(proposal_id));
            self.accepted_value.replace(Some(value.clone()));
            info!(
                "Acceptor: Accepted proposal {} with value '{}'",
                proposal_id, value
            );
            return true;
        }
        warn!("Acceptor: Rejected accept request for {}", proposal_id);
        false
    }
}
export!(MyAcceptor with_types_in self);
