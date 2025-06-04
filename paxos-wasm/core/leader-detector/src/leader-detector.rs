use std::cell::{Cell, RefCell};
use std::collections::HashMap;

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
use bindings::paxos::default::paxos_types::{Node, PaxosRole};

pub struct MyLeaderDetector;

impl Guest for MyLeaderDetector {
    type LeaderDetectorResource = MyLeaderDetectorResource;
}

pub struct MyLeaderDetectorResource {
    proposers: Vec<u64>,
    suspected: RefCell<HashMap<u64, bool>>,
    last_leader: Cell<Option<u64>>,
}

impl MyLeaderDetectorResource {
    // pick max ID of proposers where suspected==false
    fn recompute_leader(&self) -> Option<u64> {
        let suspected = self.suspected.borrow();
        self.proposers
            .iter()
            .filter(|&&id| !suspected.get(&id).copied().unwrap_or(false))
            .copied()
            .max()
    }
}

impl GuestLeaderDetectorResource for MyLeaderDetectorResource {
    fn new(all: Vec<Node>, _local: Node) -> Self {
        let mut proposers = Vec::new();
        let mut suspected = HashMap::new();

        // Collect only Coordinator/Proposer roles
        for node in &all {
            if matches!(node.role, PaxosRole::Coordinator | PaxosRole::Proposer) {
                proposers.push(node.node_id);
                suspected.insert(node.node_id, false);
            }
        }

        // Compute initial leader
        let initial = {
            let mut max_id = None;
            for &id in &proposers {
                if max_id.map_or(true, |m| id > m) {
                    max_id = Some(id);
                }
            }
            max_id
        };
        if let Some(id) = initial {
            logger::log_warn(&format!("[Leader Detector] Initialized with leader {}", id));
        }

        MyLeaderDetectorResource {
            proposers,
            suspected: RefCell::new(suspected),
            last_leader: Cell::new(initial),
        }
    }

    fn suspect(&self, node: u64) -> Option<u64> {
        if !self.proposers.contains(&node) {
            return None;
        }

        // Mark this node as suspected
        self.suspected.borrow_mut().insert(node, true);

        // Recompute highest‐ID alive
        let new_leader = self.recompute_leader();
        if new_leader != self.last_leader.get() {
            if let Some(id) = new_leader {
                logger::log_error(&format!(
                    "[Leader Detector] Leader is down, new leader is {}",
                    id
                ));
            }
            self.last_leader.set(new_leader);
            new_leader
        } else {
            None
        }
    }

    fn restore(&self, node: u64) -> Option<u64> {
        if !self.proposers.contains(&node) {
            return None;
        }

        // Mark this node as not suspected
        self.suspected.borrow_mut().insert(node, false);

        // Recompute highest‐ID alive
        let new_leader = self.recompute_leader();
        if new_leader != self.last_leader.get() {
            if let Some(id) = new_leader {
                logger::log_warn(&format!(
                    "[Leader Detector] Leader restored, new leader is {}",
                    id
                ));
            }
            self.last_leader.set(new_leader);
            new_leader
        } else {
            None
        }
    }
}
