#[allow(warnings)]
// mod bindings;

// bindings::wit-exports::pacakgepart1::packagepart2::theexportinterface




// bindings::packagepart1::packagepart2::theimportinterface::thefunction
// use bindings::paxos::acceptor::funcs::handleaccept;
// use bindings::paxos::acceptor::funcs::handleprepare;

// use std::collections::HashMap;
// use rand::Rng;

mod bindings;

use bindings::exports::paxos::proposer::proposerinterface::{Guest, GuestProposerresource};
use std::{cell::{Cell, RefCell}, collections::HashMap};
// use bindings::exports::paxos::proposer::types;
use bindings::exports::paxos::proposer::types::{Prepare, Promise, Accept};

struct MyState {
    id: Cell<u32>,
    crnd: Cell<u32>,
    numnodes: Cell<u32>,
    clientvalue: Cell<u32>,
    promises: RefCell<HashMap<u32, bool>>
}

impl GuestProposerresource for MyState {
    fn new(id: u32, numnodes: u32) -> MyState {
        MyState {
            id: Cell::new(id),
            crnd: Cell::new(id),
            numnodes: Cell::new(numnodes),
            clientvalue: Cell::new(123),
            promises: RefCell::new(HashMap::new()), 
        }
    }

    fn printhi(&self) -> String {
        "Hello host form proposer component".to_string()
    }

    fn makeprepare(&self) -> Prepare {
        let prepare = Prepare {
            from: self.id.get(),
            crnd: self.crnd.get()
        };
        prepare
    }

    fn handlepromise(&self, promise: Promise) -> Accept {
        if promise.rnd != self.crnd.get() {
            return Accept {
                from: 0,
                rnd: 0,
                val: 9999.to_string(),
            };
        }

        // Ensure safe modification of `promises`
        {
            let mut promises = self.promises.borrow_mut();
            promises.insert(promise.from, true);
        } // Drop mutable borrow here

        let num_promises = self.promises.borrow().len(); // Safe immutable borrow

        if num_promises >= self.numnodes.get().try_into().unwrap() {
            return Accept {
                from: self.id.get(),
                rnd: self.crnd.get(),
                val: self.clientvalue.get().to_string(),
            };
        }

        Accept {
            from: 0,
            rnd: 0,
            val: 9999.to_string(),
        }
    }

    fn increasecrnd(&self) {
        self.crnd.set(self.crnd.get() + self.numnodes.get());
    }

    fn getnumpromises(&self) -> u32 {
        self.promises.borrow().len().try_into().unwrap()
    }

    fn requestvalue(&self, value: u32) {
        self.clientvalue.set(value);
    }

}

struct Component;

impl Guest for Component {
    
    type Proposerresource = MyState;
}

bindings::export!(Component with_types_in bindings);