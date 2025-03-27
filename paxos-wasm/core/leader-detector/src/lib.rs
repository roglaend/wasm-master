#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::Cell;
use std::sync::Mutex;
use std::sync::Arc;
use std::collections::HashMap;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit/paxos.wit",
        world: "leader-detector-world",
    });
}

bindings::export!(MyLeaderDetector with_types_in bindings);

use crate::bindings::exports::paxos::default::leader_detector::{
    Guest, GuestLeaderDetectorResource,
};
// use crate::bindings::paxos::default::logger;
use bindings::paxos::default::{network, logger};
pub struct MyLeaderDetector;

impl Guest for MyLeaderDetector {
    type LeaderDetectorResource = MyLeaderDetectorResource;
}

/// Our learner now uses a BTreeMap to store learned values per slot.
/// This ensures that each slot only has one learned value and that the entries remain ordered.
pub struct MyLeaderDetectorResource {
    nodes: Vec<u64>,
    suspected: Arc<Mutex<HashMap<u64, bool>>>,
    leader: Cell<u64>,
}

impl GuestLeaderDetectorResource for MyLeaderDetectorResource {
    fn new(nodes: Vec<u64>, local_node_id: u64) -> Self {
        let mut suspected = HashMap::with_capacity(nodes.len());
        for node in &nodes {
            suspected.insert(*node, false);
        }
        logger::log_error(&format!("Leader detector initialized with nodes {:?}", nodes));
        let leader = find_max_false(&suspected);
        logger::log_warn(&format!("Leader detector initialized with leader {}", leader));
        Self {  
            nodes,
            suspected: Arc::new(Mutex::new(suspected)),
            leader: Cell::new(leader),
        }
    }

    fn suspect(&self, node: u64) -> Option<u64> {
        if !self.nodes.contains(&node) {
            panic!("Node {} is not in the list of nodes", node);
        }
        let mut suspected = self.suspected.lock().unwrap();
        suspected.insert(node, true);
        let leader = find_max_false(&suspected);
        if leader != self.leader.get() {
            logger::log_error(&format!("Leader is down - New leader is {}", leader));
            self.leader.set(leader);
            Some(leader)
        } else {
            None
        }
    }

    fn restore(&self, node: u64) -> Option<u64> {
        if !self.nodes.contains(&node) {
            panic!("Node {} is not in the list of nodes", node);
        }
        let mut suspected = self.suspected.lock().unwrap();
        suspected.insert(node, false);
        let leader = find_max_false(&suspected);
        if leader != self.leader.get() {
            self.leader.set(leader);
            Some(leader)
        } else {
            None
        }
    }

    fn nodes(&self) -> Vec<u64> {
        self.nodes.clone()
    }
}

/// Helper function that finds the maximum node id that is not suspected.
/// Returns 0 if all nodes are suspected.
fn find_max_false(suspected: &HashMap<u64, bool>) -> u64 {
    suspected
        .iter()
        .filter(|&(_, &is_suspected)| !is_suspected)
        .map(|(&id, _)| id)
        .max()
        .unwrap_or(0)
}
