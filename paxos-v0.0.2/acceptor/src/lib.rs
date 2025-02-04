// #[allow(warnings)]
mod bindings;

// wit_bindgen::generate!({
//     // the name of the world in the `*.wit` input file
//     world: "acceptor",
// });

use bindings::exports::paxos::acceptor::acceptorinterface::{Guest, GuestAcceptorresource};

use std::cell::{Cell, RefCell};
use bindings::exports::paxos::acceptor::types::{Prepare, Promise, Accept, Learn};

struct MyState {
    id: Cell<u32>,
    rnd: Cell<u32>,
    crnd: Cell<u32>,
    vrnd: Cell<u32>,
    vval: RefCell<String>,
}



impl GuestAcceptorresource for MyState {

    fn new(id: u32) -> MyState {
        MyState {
            id: Cell::new(id),
            rnd: Cell::new(0),
            crnd: Cell::new(0),
            vrnd: Cell::new(0),
            vval: RefCell::new("".to_string()),
        }
    }

    fn handleprepare(&self, prepare: Prepare) -> Promise {
        if prepare.crnd <= self.rnd.get() {
            return Promise {
                to: 0,
                from: 0,
                rnd: 0,
                vrnd: 0,
                vval: 0.to_string(),
            };
        }

        self.rnd.set(prepare.crnd);

        Promise {
            to: prepare.from,
            from: self.id.get(),
            rnd: self.rnd.get(),
            vrnd: self.vrnd.get(),
            vval: self.vval.borrow().to_string(),
        }
    }

    fn handleaccept(&self, accept: Accept) -> Learn {
        if accept.rnd < self.rnd.get() || accept.rnd == self.vrnd.get() {
            return Learn {
                from: 0,
                rnd: 0,
                val: 0.to_string(),
            };
        }

        self.rnd.set(accept.rnd);
        self.vrnd.set(accept.rnd);
        self.vval.replace(accept.val);

        Learn {
            from: self.id.get(),
            rnd: self.rnd.get(),
            val: self.vval.borrow().to_string(),
        }
        
    }
}

struct Component;

impl Guest for Component {
    
    type Acceptorresource = MyState;
}

bindings::export!(Component with_types_in bindings);
// export!(AcceptorState);