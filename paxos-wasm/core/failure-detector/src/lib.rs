#![allow(unsafe_op_in_unsafe_fn)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "failure-detector-world",
    });
}

bindings::export!(MyFailureDetector with_types_in bindings);

use crate::bindings::exports::paxos::default::failure_detector::{
    Guest, GuestFailureDetectorResource,
};
use bindings::paxos::default::paxos_types::Node;
use bindings::paxos::default::{leader_detector, logger};

pub struct MyFailureDetector;

impl Guest for MyFailureDetector {
    type FailureDetectorResource = MyFailureDetectorResource;
}

pub struct MyFailureDetectorResource {
    node_id: u64,
    nodes: Vec<u64>,
    ld: Arc<leader_detector::LeaderDetectorResource>,
    alive: Arc<Mutex<HashMap<u64, bool>>>,
    suspected: Arc<Mutex<HashMap<u64, bool>>>,
}

// TODO: Change this to take in and use the Node type more than just the node_id
impl GuestFailureDetectorResource for MyFailureDetectorResource {
    fn new(node_id: u64, nodes: Vec<Node>, _delta: u64) -> Self {
        let mut node_ids: Vec<u64> = nodes.iter().map(|node| node.node_id).collect();
        // Add yourself to the list of nodes
        node_ids.push(node_id);

        // TODO: Add the Node type as argument instead of node_id.
        // TODO: We can then filter based on PaxosRole to only consider the relevant nodes for new leaders.
        let ld = Arc::new(leader_detector::LeaderDetectorResource::new(
            &node_ids, node_id,
        ));

        Self {
            ld,
            node_id,
            nodes: node_ids,
            alive: Arc::new(Mutex::new(HashMap::new())),
            suspected: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn checker(&self) -> Option<u64> {
        logger::log_info("[Failure Detector] Checker function called.");
        let mut alive = self.alive.lock().unwrap();
        let mut suspected = self.suspected.lock().unwrap();

        // Check if any alive node is also suspected.
        // TODO : Get this back to host to increase the delta
        let _increase_delta = alive.keys().any(|node| suspected.get(node) == Some(&true));

        logger::log_info(&format!("Current suspected nodes: {:?}", suspected.keys()));
        logger::log_info(&format!("Current alive nodes: {:?}", alive.keys()));

        let new_lead = self.nodes.iter().fold(None, |acc, node| {
            if !alive.contains_key(node) && !suspected.contains_key(node) {
                // Node not known to be alive or suspected.
                suspected.insert(*node, true);
                let lead = self.ld.suspect(*node);
                alive.remove(node);
                Some(lead)
            } else if alive.contains_key(node) && suspected.contains_key(node) {
                // Node is alive but also marked as suspected: restore it.
                suspected.remove(node);
                alive.remove(node);
                Some(self.ld.restore(*node))
            } else {
                // In all other cases, remove the node from alive.
                alive.remove(node);
                acc
            }
        });
        new_lead?
    }

    fn heartbeat(&self, node: u64) {
        let mut alive = self.alive.lock().unwrap();
        alive.insert(node, true);

        // TODO: Quixfix for always ensuring yourself is alive
        //* Is there a better way than this? - Filip */
        alive.insert(self.node_id, true);
    }
}
