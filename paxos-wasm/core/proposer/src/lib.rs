#![allow(unsafe_op_in_unsafe_fn)]

use bindings::exports::paxos::default::proposer::ClientProposal;
// use bindings::paxos::default::types::{Accept, Learn};
use std::cell::{Cell, Ref, RefCell};
use std::collections::{HashMap, VecDeque};
use std::string;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit/paxos.wit",
        world: "proposer-world",
    });
}

bindings::export!(MyProposer with_types_in bindings);

use crate::bindings::exports::paxos::default::proposer::{
    Guest, GuestProposerResource, Proposal, ProposeResult, ProposerState, Promise, Prepare, Learn, Accept
};
use crate::bindings::paxos::default::logger;

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
    adu: Cell<u64>,
    pending_client_requests: RefCell<VecDeque<ClientProposal>>,
    in_flight_accepts: RefCell<VecDeque<Accept>>,
    promises: RefCell<Vec<Promise>>,
    learns: RefCell<HashMap<u64, Vec<Learn>>>
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
            adu: Cell::new(0),
            pending_client_requests: RefCell::new(VecDeque::new()),
            promises: RefCell::new(Vec::new()),
            learns: RefCell::new(HashMap::new()),
            in_flight_accepts: RefCell::new(VecDeque::new())
        }
    }

    fn is_leader(&self,) -> bool {
        self.is_leader.get()
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
            logger::log_warn("Proposer: Not leader—proposal rejected.");
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

        logger::log_info(&format!(
            "Proposer: Proposing value '{}' for slot {} with ballot {}",
            proposal.client_proposal.value,
            slot,
            self.current_ballot.get()
        ));
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
        logger::log_info(&format!(
            "Proposer: Became leader with ballot {}",
            new_ballot
        ));
        true
    }

    /// Relinquish leadership by marking the proposer as not leader.
    fn resign_leader(&self) -> bool {
        if self.is_leader.get() {
            self.is_leader.set(false);
            logger::log_info("Proposer: Resigned leadership.");
            true
        } else {
            logger::log_warn("Proposer: Already not leader.");
            false
        }
    }

    fn enqueue_client_request(&self, req: ClientProposal) -> bool {
        self.pending_client_requests.borrow_mut().push_back(req);
        logger::log_debug("[Core Proposer] Enqueued client request.");
        true
    }

    fn create_prepare(&self, sender: String) -> Prepare {
        let prepare = Prepare {
            sender: sender.to_string(),
            slot: self.adu.get() + 1,
            ballot: self.current_ballot.get()
        };
        prepare
    }

    fn register_promise(&self, promise: Promise) -> bool {
        if promise.ballot != self.current_ballot.get() {
            return  false;
        }
        self.promises.borrow_mut().push(promise);
        true
    }

    // TODO : should process similar to PrepareQF which returns all slots > prepare.slot acceptors have accepted
    // TODO: + also the last logic inside prpoposer runphaseone which appends them to a priority queue to get new leader uptospeed
    fn process_promises(&self, quorum: u64) -> bool {
        if self.promises.borrow().len() < quorum as usize{
            logger::log_info("[Core Proposer] No quorum of promises");
            return false;
        }
        true
    }

    fn create_accept(&self, sender: String) -> Option<Accept> {
        // if !self.is_leader.get() {
        //     logger::log_warn("[Core Proposer] Not leader; proposal creation aborted.");
        //     return None;
        // }

        let client_req = match self.pending_client_requests.borrow_mut().pop_front() {
            Some(cr) => cr,
            None => {
                logger::log_info("[Core Proposer] No pending client requests available.");
                return None;
            }
        };

        // Assign the next available slot.
        let slot = self.current_slot.get();
        self.current_slot.set(slot + 1);

        let prop = Accept {
            sender,
            ballot: self.current_ballot.get(),
            slot,
            value: client_req.value,
        };

        // Create an in-flight entry with accept
        self.in_flight_accepts.borrow_mut().push_back(prop.clone());

        logger::log_info(&format!(
            "[Core Proposer] Created proposal: ballot = {:?}, slot = {}, value = '{}'.",
            prop.ballot, prop.slot, prop.value,
        ));

        Some(prop)
    }

    fn register_learn(&self, learn: Learn) -> bool {
        self.learns
            .borrow_mut()
            .entry(learn.slot)
            .or_default()
            .push(learn);
        true
    }

    // TODO : process similar to AcceptQF
    fn process_learns(&self, quorum: u64) -> Option<Learn> {
        let mut queue = self.in_flight_accepts.borrow_mut();
        if let Some(next_accept) = queue.pop_front() {
            logger::log_info(&format!(
                "[Core Proposer] Checking for learns for slot = {}",
                next_accept.slot
            ));
            
            let learns_map = self.learns.borrow();
            if let Some(learns_vec) = learns_map.get(&next_accept.slot) {
                if learns_vec.len() < quorum as usize {
                    // Push back since quorum not met
                    logger::log_info(&format!(
                        "[Core Proposer] Quorum not reached yet for slot = {}",
                        next_accept.slot
                    ));
                    drop(queue);
                    self.in_flight_accepts.borrow_mut().push_front(next_accept);
                    return None;
                }
                return learns_vec.first().cloned();
            } else {
                logger::log_info(&format!(
                    "[Core Proposer] No learns yet for slot = {}",
                    next_accept.slot
                ));
                drop(queue);
                self.in_flight_accepts.borrow_mut().push_front(next_accept);
            }
        }
        None
    }

    fn advance_adu(&self) {
        self.adu.set(self.adu.get() + 1);
    }
}
