use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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
    nodes: HashMap<u64, Node>,
    relevant_node_ids: Vec<u64>,
    suspected: Arc<Mutex<HashMap<u64, bool>>>,
    leader: Cell<u64>,
}

impl GuestLeaderDetectorResource for MyLeaderDetectorResource {
    fn new(nodes: Vec<Node>, _local_node_id: u64) -> Self {
        let mut node_map = HashMap::new();
        let mut relevant_ids = Vec::new();

        for node in nodes {
            if matches!(node.role, PaxosRole::Coordinator | PaxosRole::Proposer) {
                relevant_ids.push(node.node_id);
            }
            node_map.insert(node.node_id, node);
        }

        let suspected = relevant_ids.iter().map(|&id| (id, false)).collect();
        let leader = find_max_false(&suspected);

        logger::log_info(&format!(
            "[Leader Detector] Relevant nodes: {:?}, Initial leader: {}",
            relevant_ids, leader
        ));

        Self {
            nodes: node_map,
            relevant_node_ids: relevant_ids,
            suspected: Arc::new(Mutex::new(suspected)),
            leader: Cell::new(leader),
        }
    }

    fn suspect(&self, node: u64) -> Option<u64> {
        if !self.relevant_node_ids.contains(&node) {
            return None;
        }

        let mut suspected = self.suspected.lock().unwrap();
        suspected.insert(node, true);

        let leader = find_max_false(&suspected);
        if leader != self.leader.get() {
            logger::log_error(&format!(
                "[Leader Detector] Node {} suspected. New leader: {}",
                node, leader
            ));
            self.leader.set(leader);
            Some(leader)
        } else {
            None
        }
    }

    fn restore(&self, node: u64) -> Option<u64> {
        if !self.relevant_node_ids.contains(&node) {
            return None;
        }

        let mut suspected = self.suspected.lock().unwrap();
        suspected.insert(node, false);

        let leader = find_max_false(&suspected);
        if leader != self.leader.get() {
            logger::log_error(&format!(
                "[Leader Detector] Node {} restored. New leader: {}",
                node, leader
            ));
            self.leader.set(leader);
            Some(leader)
        } else {
            None
        }
    }

    fn nodes(&self) -> Vec<u64> {
        self.relevant_node_ids.clone()
    }
}

fn find_max_false(suspected: &HashMap<u64, bool>) -> u64 {
    suspected
        .iter()
        .filter(|&(_, &sus)| !sus)
        .map(|(&id, _)| id)
        .max()
        .unwrap_or(0)
}
