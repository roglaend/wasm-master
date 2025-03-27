#![allow(unsafe_op_in_unsafe_fn)]

use std::sync::Mutex;
use std::{cell::RefCell, sync::Arc};
use std::collections::{BTreeMap, HashMap};

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit/paxos.wit",
        world: "failure-detector-world",
    });
}

bindings::export!(MyFailureDetector with_types_in bindings);

use bindings::paxos::default::{leader_detector, logger, network};

use crate::bindings::exports::paxos::default::failure_detector::{
    Guest, GuestFailureDetectorResource,
};

pub struct MyFailureDetector;

impl Guest for MyFailureDetector {
    type FailureDetectorResource = MyFailureDetectorResource;
}

/// Our learner now uses a BTreeMap to store learned values per slot.
/// This ensures that each slot only has one learned value and that the entries remain ordered.
pub struct MyFailureDetectorResource {
    node_id: u64,
    nodes: Vec<u64>,
    ld: Arc<leader_detector::LeaderDetectorResource>,
    alive: Arc<Mutex<HashMap<u64, bool>>>,
    suspected: Arc<Mutex<HashMap<u64, bool>>>, 
}

impl GuestFailureDetectorResource for MyFailureDetectorResource {
    /// Constructor: Initialize an empty BTreeMap.
    fn new(node_id: u64, nodes: Vec<network::Node>, delta: u64) -> Self {
        let mut node_ids: Vec<u64> = nodes.iter().map(|node| node.id).collect();
        // Add yourself to the list of nodes
        node_ids.push(node_id);

        let ld = Arc::new(leader_detector::LeaderDetectorResource::new(&node_ids, node_id));
        
        Self {
            ld,
            node_id,
            nodes: node_ids,
            alive: Arc::new(Mutex::new(HashMap::new())),
            suspected: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Returns the current state as a list of learned entries, sorted by slot.
    fn checker(&self) -> Option<u64> {
        logger::log_info("Failure detector called");
        let mut intersection = HashMap::new();
        let mut alive = self.alive.lock().unwrap();
        let mut suspected = self.suspected.lock().unwrap();
        for node in alive.keys() {
            if suspected.get(node) == Some(&true) {
                intersection.insert(node, true);
            }
        }

        // TODO : Get this back to host to increase the delta
        let increase_delta = intersection.len() > 0;

        logger::log_info(&format!("Current suspected nodes: {:?}", suspected.keys()));
        logger::log_info(&format!("Current alive nodes: {:?}", alive.keys()));

        let mut new_lead = None;
        for node in self.nodes.iter() {
            if !alive.contains_key(node) && !suspected.contains_key(node) {
                suspected.insert(*node, true);
                new_lead = Some(self.ld.suspect(*node));
                alive.remove(node);
            } else if alive.contains_key(node) && suspected.contains_key(node) {
                suspected.remove(node);
                alive.remove(node);
                new_lead = Some(self.ld.restore(*node));
            } else {
                alive.remove(node);
            }
        }
        new_lead?
    }

    fn heartbeat(&self, node: u64) {
        let mut alive = self.alive.lock().unwrap();
        alive.insert(node, true);

        // TODO: Quixfix for always ensuring yourself is alive
        alive.insert(self.node_id, true);
    }
}
