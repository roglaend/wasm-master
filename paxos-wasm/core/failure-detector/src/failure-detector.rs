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
use bindings::paxos::default::paxos_types::{Node, PaxosRole};
use bindings::paxos::default::{leader_detector, logger};

pub struct MyFailureDetector;

impl Guest for MyFailureDetector {
    type FailureDetectorResource = MyFailureDetectorResource;
}

pub struct MyFailureDetectorResource {
    node: Node,
    nodes: HashMap<u64, Node>,
    ld: Arc<leader_detector::LeaderDetectorResource>,
    alive: Arc<Mutex<HashMap<u64, bool>>>,
    suspected: Arc<Mutex<HashMap<u64, bool>>>,
}

impl GuestFailureDetectorResource for MyFailureDetectorResource {
    fn new(node: Node, mut nodes: Vec<Node>, _delta: u64) -> Self {
        nodes.push(node.clone());

        let node_map = nodes
            .iter()
            .map(|n| (n.node_id, n.clone()))
            .collect::<HashMap<_, _>>();

        let ld = Arc::new(leader_detector::LeaderDetectorResource::new(
            &nodes.clone(),
            node.node_id,
        ));

        Self {
            node,
            nodes: node_map,
            ld,
            alive: Arc::new(Mutex::new(HashMap::new())),
            suspected: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn checker(&self) -> Option<u64> {
        logger::log_info("[Failure Detector] Checker function called.");
        let mut alive = self.alive.lock().unwrap();
        let mut suspected = self.suspected.lock().unwrap();

        let _increase_delta = alive.keys().any(|id| suspected.get(id) == Some(&true));

        for node_id in self.nodes.keys() {
            let id = *node_id;

            if !alive.contains_key(&id) && !suspected.contains_key(&id) {
                suspected.insert(id, true);
                alive.remove(&id);
                if let Some(leader) = self.ld.suspect(id) {
                    return Some(leader);
                }
            } else if alive.contains_key(&id) && suspected.contains_key(&id) {
                suspected.remove(&id);
                alive.remove(&id);
                if let Some(leader) = self.ld.restore(id) {
                    return Some(leader);
                }
            } else {
                alive.remove(&id);
            }
        }

        None
    }

    fn heartbeat(&self, node_id: u64) {
        let mut alive = self.alive.lock().unwrap();
        alive.insert(node_id, true);
        alive.insert(self.node.node_id, true); // Ensure self is marked alive
    }
}
