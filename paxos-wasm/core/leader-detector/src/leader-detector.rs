#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "leader-detector-world",
    });
}

bindings::export!(MyLeaderDetector with_types_in bindings);

use crate::bindings::exports::paxos::default::leader_detector::{
    Guest, GuestLeaderDetectorResource,
};
use crate::bindings::paxos::default::logger;
use bindings::paxos::default::paxos_types::Node;
use bindings::paxos::default::paxos_types::PaxosRole;
pub struct MyLeaderDetector;

impl Guest for MyLeaderDetector {
    type LeaderDetectorResource = MyLeaderDetectorResource;
}

pub struct MyLeaderDetectorResource {
    nodes: Vec<u64>,
    suspected: Arc<Mutex<HashMap<u64, bool>>>,
    leader: Cell<u64>,
}

// TODO: Change this to take in Node types instead of ids
impl GuestLeaderDetectorResource for MyLeaderDetectorResource {
    fn new(nodes: Vec<Node>, _local_node_id: Node) -> Self {
        let mut suspected = HashMap::with_capacity(nodes.len());

        let mut relevant_nodes = Vec::new();
        for node in &nodes {
            if matches!(node.role, PaxosRole::Coordinator | PaxosRole::Proposer) {
                suspected.insert(node.node_id, false);
                relevant_nodes.push(node.node_id);
            }
        }
        let leader = find_max_false(&suspected);
        logger::log_warn(&format!(
            "[Leader Detector] Initialized with leader {}",
            leader
        ));

        Self {
            nodes: relevant_nodes,
            suspected: Arc::new(Mutex::new(suspected)),
            leader: Cell::new(leader),
        }
    }

    fn suspect(&self, node: u64) -> Option<u64> {
        if !self.nodes.contains(&node) {
            // Leader detector does not care about non-relevant nodes
            return None;
        }
        let mut suspected = self.suspected.lock().unwrap();
        suspected.insert(node, true);
        let leader = find_max_false(&suspected);
        if leader != self.leader.get() {
            logger::log_error(&format!(
                "[Leader Detector] Leader is down - New leader is {}",
                leader
            ));
            self.leader.set(leader);
            Some(leader)
        } else {
            None
        }
    }

    fn restore(&self, node: u64) -> Option<u64> {
        if !self.nodes.contains(&node) {
            // Leader detector does not care about non-relevant nodes
            return None;
        }
        let mut suspected = self.suspected.lock().unwrap();
        suspected.insert(node, false);
        let leader = find_max_false(&suspected);
        if leader != self.leader.get() {
            logger::log_error(&format!(
                "[Leader Detector] Leader restored - Restored leader is {}",
                leader
            ));
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
/// Returns 0 if all nodes are suspected. // TODO: Return an option for clarity?
fn find_max_false(suspected: &HashMap<u64, bool>) -> u64 {
    suspected
        .iter()
        .filter(|&(_, &is_suspected)| !is_suspected)
        .map(|(&id, _)| id)
        .max()
        .unwrap_or(0)
}
