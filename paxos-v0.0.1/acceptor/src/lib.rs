#[allow(warnings)]
mod bindings;


use bindings::exports::paxos::acceptor::funcs::Guest;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

#[derive(Default)]
pub struct AcceptorState {
    promised_n: Option<u32>,
    accepted_n: Option<u32>,
    accepted_value: Option<String>,
}

lazy_static::lazy_static! {
    static ref ACCEPTOR_STATES: Arc<Mutex<HashMap<u32, AcceptorState>>> = Arc::new(Mutex::new(HashMap::new()));
}

impl Guest for AcceptorState {
    fn handleprepare(id: u32, proposal_number: u32) -> (bool, Option<String>) {
        let mut states = ACCEPTOR_STATES.lock().unwrap();
        let state = states.entry(id).or_insert_with(AcceptorState::default);

        if state.promised_n.is_none() || proposal_number > state.promised_n.unwrap() {
            state.promised_n = Some(proposal_number);
            return (true, state.accepted_value.clone());
        }

        (false, None)
    }

    fn handleaccept(id: u32, proposal_number: u32, value: String) -> bool {
        let mut states = ACCEPTOR_STATES.lock().unwrap();
        let state = states.entry(id).or_insert_with(AcceptorState::default);
        
        if state.promised_n.is_none() || proposal_number >= state.promised_n.unwrap() {
            state.accepted_n = Some(proposal_number);
            state.accepted_value = Some(value);
            return true;
        }
        
        false
    }
}

bindings::export!(AcceptorState with_types_in bindings);
