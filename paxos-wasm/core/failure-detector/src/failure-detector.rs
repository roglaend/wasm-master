#![allow(unsafe_op_in_unsafe_fn)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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

#[derive(Clone, Copy, PartialEq)]
enum FailureState {
    Alive,
    Suspected,
    Dead,
}

pub struct MyFailureDetector;

impl Guest for MyFailureDetector {
    type FailureDetectorResource = MyFailureDetectorResource;
}

pub struct MyFailureDetectorResource {
    node_id: u64,
    node_ids: Vec<u64>,
    ld: Arc<leader_detector::LeaderDetectorResource>,

    // Tracks each node’s state: Alive, Suspected, or Dead.
    state_map: Arc<Mutex<HashMap<u64, FailureState>>>,

    // Records which nodes sent a heartbeat since last check.
    seen_heartbeat: Arc<Mutex<HashMap<u64, bool>>>,
}

impl GuestFailureDetectorResource for MyFailureDetectorResource {
    fn new(node: Node, mut nodes: Vec<Node>, _delta: u64) -> Self {
        let mut node_ids: Vec<u64> = nodes.iter().map(|n| n.node_id).collect();
        node_ids.push(node.node_id);
        nodes.push(node.clone());

        let ld = Arc::new(leader_detector::LeaderDetectorResource::new(&nodes, &node));

        // Initialize all nodes as Alive.
        let mut map = HashMap::new();
        for id in &node_ids {
            map.insert(*id, FailureState::Alive);
        }

        Self {
            node_id: node.node_id,
            node_ids,
            ld,
            state_map: Arc::new(Mutex::new(map)),
            seen_heartbeat: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn checker(&self) -> Option<u64> {
        logger::log_info("[Failure Detector] checker() called.");

        let mut state_map = self.state_map.lock().unwrap();
        let mut seen = self.seen_heartbeat.lock().unwrap();
        let mut maybe_new_leader = None;

        // TODO: Still consider adding a dynamic failure check delta.

        for &id in &self.node_ids {
            let current = state_map.get(&id).copied().unwrap_or(FailureState::Alive);

            if seen.contains_key(&id) {
                // Got a heartbeat: if node was Suspected or Dead, restore it.
                if current != FailureState::Alive {
                    logger::log_warn(&format!("[Failure Detector] Node {} restored.", id));
                    if let Some(new_lead) = self.ld.restore(id) {
                        maybe_new_leader = Some(new_lead);
                    }
                }
                state_map.insert(id, FailureState::Alive);
            } else {
                // No heartbeat this round.
                match current {
                    FailureState::Alive => {
                        // First miss, mark Suspected.
                        logger::log_warn(&format!("[Failure Detector] Node {} suspected.", id));
                        state_map.insert(id, FailureState::Suspected);
                    }
                    FailureState::Suspected => {
                        // Second miss, mark Dead and notify leader detector.
                        logger::log_error(&format!("[Failure Detector] Node {} dead.", id));
                        state_map.insert(id, FailureState::Dead);
                        if let Some(new_lead) = self.ld.suspect(id) {
                            maybe_new_leader = Some(new_lead);
                        }
                    }
                    FailureState::Dead => {
                        // Already dead, do nothing.
                    }
                }
            }
        }

        // Clear for next round.
        seen.clear();
        maybe_new_leader
    }

    fn heartbeat(&self, node: u64) {
        let mut seen = self.seen_heartbeat.lock().unwrap();
        seen.insert(node, true);
        seen.insert(self.node_id, true);
    }
}
