// #[allow(warnings)]
mod bindings;

// wit_bindgen::generate!({
//     // the name of the world in the `*.wit` input file
//     world: "acceptor",
// });

use bindings::exports::paxos::acceptor::acceptorinterface::{Guest, GuestAcceptorresource, AcceptorresourceBorrow};

use std::fmt;
use std::cell::{Cell, RefCell};
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use bindings::exports::paxos::acceptor::types::{Prepare, Promise, Accept, Learn};

struct MyState {
    id: Cell<u32>,
    rnd: Cell<u32>,
    vrnd: Cell<u32>,
    vval: RefCell<String>,
}



impl GuestAcceptorresource for MyState {

    fn new(id: u32) -> MyState {
        MyState {
            id: Cell::new(id),
            rnd: Cell::new(0),
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


enum Request {
    Prepare(Prepare),
    Accept(Accept),
}

impl FromStr for Request {
    type Err = &'static str;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = input.split(':').collect();
        
        match parts.as_slice() {
            ["prepare", from, crnd] => {
                let from_val = parse_key_value(from, "from")?;
                let crnd_val = parse_key_value(crnd, "crnd")?;
                Ok(Request::Prepare(Prepare { from: from_val, crnd: crnd_val }))
            },
            ["accept", from, rnd, val] => {
                let from_val = parse_key_value(from, "from")?;
                let rnd_val = parse_key_value(rnd, "rnd")?;
                let val_str = parse_value(val, "val")?;
                Ok(Request::Accept(Accept { from: from_val, rnd: rnd_val, val: val_str }))
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

enum Message {
    Promise(Promise),
    Learn(Learn),
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Message::Promise(p) => {
                if p.rnd == 0 {
                    write!(f, "promise:to=0:from=0:rnd=0:vrnd=0:vval=0")
                } else {
                    write!(f, "promise:to={}:from={}:rnd={}:vrnd={}:vval={}", 
                           p.to, p.from, p.rnd, p.vrnd, p.vval)
                }
                // write!(f, "promise:to={}:from={}:rnd={}:vrnd={}:vval={}", 
                //        p.to, p.from, p.rnd, p.vrnd, p.vval)
            }
            Message::Learn(l) => {
                write!(f, "learn:from={}:rnd={}:val={}", l.from, l.rnd, l.val)
            }
        }
    }
}


struct Component {
    test: u32,
}

impl Guest for Component {
    
    type Acceptorresource = MyState;


    // format on request
    // promise:to=123:from=456:rnd=789:vrnd=1011:vval=hello
    // learn:from=321:rnd=654:val=world
    // prepare:from=123:crnd=456
    // accept:from=123:rnd=456:val=hello

    fn handle_request(v: AcceptorresourceBorrow<'_>, req: String) -> (String, String) {
        let request = req.parse::<Request>().unwrap();
        
        match request {
            Request::Prepare(prepare) => {
                let v: &MyState = v.get();
                let promise = v.handleprepare(prepare);
                let response = Message::Promise(promise).to_string();
                (response, "RETURN".to_string())
            },
            Request::Accept(accept) => {
                let v: &MyState = v.get();
                let learn = v.handleaccept(accept);
                let response = Message::Learn(learn).to_string();
                (response, "SEND_TO_ALL_CLIENTS".to_string())
            },
        }

    }



//     fn handle_request(v: AcceptorresourceBorrow<'_>, req: Request) -> Response {
//         match req {
//             Request::Prepare(prepare) => {
//                 let v: &MyState = v.get();
//                 let promise = v.handleprepare(prepare);
//                 Response {action: Action::Return, response: ResponseRequest::Promise(promise)}
//             }
//             Request::Accept(accept) => {
//                 let v: &MyState = v.get();
//                 let learn = v.handleaccept(accept);
//                 Response {action: Action::CallAllLearners, response: ResponseRequest::Learn(learn)}
//             }
//             Request::Value(value)  => {
//                 Response {action: Action::Return, response: ResponseRequest::Test(value)}
//             }
//         }        
//     }
}

bindings::export!(Component with_types_in bindings);
// export!(AcceptorState);