#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::RefCell;
use std::collections::HashMap;

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
    local_id: u64,
    all_nodes: Vec<Node>,
    leader_detector: leader_detector::LeaderDetectorResource,
    state_map: RefCell<HashMap<u64, FailureState>>,
    seen: RefCell<HashMap<u64, bool>>,
}

impl GuestFailureDetectorResource for MyFailureDetectorResource {
    fn new(local: Node, others: Vec<Node>, _delta: u64) -> Self {
        let mut all = others.clone();
        all.push(local.clone());

        // TODO: Make use of delta

        let ld = leader_detector::LeaderDetectorResource::new(&all, &local);

        let mut map = HashMap::new();
        for node in &all {
            map.insert(node.node_id, FailureState::Alive);
        }

        MyFailureDetectorResource {
            local_id: local.node_id,
            all_nodes: all,
            leader_detector: ld,
            state_map: RefCell::new(map),
            seen: RefCell::new(HashMap::new()),
        }
    }

    fn checker(&self) -> Option<u64> {
        logger::log_info("[Failure Detector] checker() called");
        let mut states = self.state_map.borrow_mut();
        let mut seen = self.seen.borrow_mut();
        let mut new_leader = None;

        for node in &self.all_nodes {
            let id = node.node_id;
            let current = *states.get(&id).unwrap_or(&FailureState::Alive);

            // If we saw a heartbeat from this node, mark it Alive (and possibly restore)
            if seen.remove(&id).unwrap_or(false) {
                if current != FailureState::Alive {
                    logger::log_warn(&format!("[Failure Detector] Node {} restored", id));
                    // Notify leader detector that this node is back up
                    if let Some(l) = self.leader_detector.restore(id) {
                        new_leader = Some(l);
                    }
                }
                states.insert(id, FailureState::Alive);

            // No heartbeat, first miss: Suspected, second miss: Dead
            } else {
                match current {
                    FailureState::Alive => {
                        logger::log_warn(&format!("[Failure Detector] Node {} suspected", id));
                        states.insert(id, FailureState::Suspected);
                    }
                    FailureState::Suspected => {
                        logger::log_error(&format!("[Failure Detector] Node {} dead", id));
                        states.insert(id, FailureState::Dead);
                        // Notify leader detector that this node is down
                        if let Some(l) = self.leader_detector.suspect(id) {
                            new_leader = Some(l);
                        }
                    }
                    FailureState::Dead => {
                        // Already dead
                    }
                }
            }
        }

        seen.clear();
        new_leader
    }

    fn heartbeat(&self, from: u64) {
        let mut seen = self.seen.borrow_mut();
        seen.insert(from, true);
        seen.insert(self.local_id, true);
    }

    fn alive_nodes(&self) -> Vec<u64> {
        let states = self.state_map.borrow();
        self.all_nodes
            .iter()
            .filter_map(|n| {
                if states
                    .get(&n.node_id)
                    .copied()
                    .unwrap_or(FailureState::Alive)
                    != FailureState::Dead
                {
                    Some(n.node_id)
                } else {
                    None
                }
            })
            .collect()
    }
}
