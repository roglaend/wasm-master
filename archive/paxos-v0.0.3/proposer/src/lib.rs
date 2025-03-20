#[allow(warnings)]
mod bindings;

use bindings::exports::paxos::proposer::proposerinterface::{Guest, GuestProposerresource, ProposerresourceBorrow};
use std::{cell::{Cell, RefCell}, collections::HashMap};
use std::str::FromStr;
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
            crnd: Cell::new(id - 1),
            numnodes: Cell::new(numnodes),
            clientvalue: Cell::new(0),
            promises: RefCell::new(HashMap::new()), 
        }
    }

    fn makeprepare(&self, value: u32) -> Prepare {
        self.increasecrnd();
        self.requestvalue(value);

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

        let quorum: usize = ((self.numnodes.get() / 2) + 1) as usize;


        if num_promises >= quorum {
            return Accept {
                from: self.id.get(),
                rnd: self.crnd.get(),
                // val should be either clientvalue or value from majority promise
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
        self.crnd.set(self.crnd.get() + 1);
    }

    // fn getnumpromises(&self) -> u32 {
    //     self.promises.borrow().len().try_into().unwrap()
    // }

    fn requestvalue(&self, value: u32) {
        self.clientvalue.set(value);
    }

}


struct Component;

enum Request {
    ClientRequest(u32),
    Promise(Promise),
}

impl FromStr for Request {
    type Err = &'static str;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = input.split(':').collect();
        
        match parts.as_slice() {
            ["clientrequest", value] => {
                let client_value = parse_key_value(value, "value")?;
                Ok(Request::ClientRequest(client_value)) 
            },

            ["promise", to, from, rnd, vrnd, vval] => {
                let to_val = parse_key_value(to, "to")?;
                let from_val = parse_key_value(from, "from")?;
                let rnd_val = parse_key_value(rnd, "rnd")?;
                let vrnd_val = parse_key_value(vrnd, "vrnd")?;
                let vval_str = parse_value(vval, "vval")?;
                Ok(Request::Promise(Promise { to: to_val, from: from_val, rnd: rnd_val, vrnd: vrnd_val, vval: vval_str }))
            },
            _ => Err("Invalid message format"),
        }
    }
}

fn parse_key_value(input: &str, key: &str) -> Result<u32, &'static str> {
    if let Some((k, v)) = input.split_once('=') {
        if k == key {
            return v.parse::<u32>().map_err(|_| "Invalid number format");
        }
    }
    Err("Invalid key-value format")
}

fn parse_value(input: &str, key: &str) -> Result<String, &'static str> {
    if let Some((k, v)) = input.split_once('=') {
        if k == key {
            return Ok(v.to_string());
        }
    }
    Err("Invalid key-value format")
}

impl Guest for Component {
    
    type Proposerresource = MyState;

    // format on requests
    // clientrequest:value=123
    // promise:from=123:crnd=456

    fn handle_request(v: ProposerresourceBorrow<'_>, req: String) -> (String, String) {
        let request = req.parse::<Request>().unwrap();

        match request {
            Request::ClientRequest(client_request) => {
                let v: &MyState = v.get();
                let prepare = v.makeprepare(client_request);
                let prepare_str = format!("prepare:from={}:crnd={}", prepare.from, prepare.crnd);
                // v.increasecrnd();
                (prepare_str, "SEND_TO_ALL_CLIENTS_AND_HANDLE".to_string())
            }
            Request::Promise(promise) => {
                let v: &MyState = v.get();
                let accept = v.handlepromise(promise);
                // EMPTY ACCEPT = NO QUORUM
                if accept.from == 0 && accept.rnd == 0 && accept.val == "9999".to_string() {
                    // let quorum: usize = ((v.numnodes.get() / 2) + 1) as usize;
                    let num_promises = v.promises.borrow().len();
                    return (num_promises.to_string(), "NO_OP".to_string());
                } else {
                    let accept_str = format!("accept:from={}:rnd={}:val={}", accept.from, accept.rnd, accept.val);
                    (accept_str, "SEND_TO_ALL_CLIENTS".to_string())
                }
            }
        }
        
        
    }

    // fn handle_request(v: ProposerresourceBorrow<'_>, req: Request) -> Response {
    //     match req {
    //         Request::ClientRequest(client_request) => {
    //             let v: &MyState = v.get();
    //             let prepare = v.makeprepare(client_request);
    //             Response {action: Action::CallAllAcceptors, response: ResponseRequest::Prepare(prepare)}
    //         }
    //         Request::Promise(promise) => {
    //             let v: &MyState = v.get();
    //             let accept = v.handlepromise(promise);
    //             Response {action: Action::CallAllAcceptors, response: ResponseRequest::Accept(accept)}
    //         }
    //     }
    // }


}

bindings::export!(Component with_types_in bindings);