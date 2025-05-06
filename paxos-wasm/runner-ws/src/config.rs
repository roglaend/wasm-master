use std::{fs, path::Path};
use yaml_rust::YamlLoader;

use crate::bindings::paxos::default::{
    network_types::PaxosRole,
    paxos_types::{Node, RunConfig},
};

pub struct Config {
    pub node: Node,
    pub remote_nodes: Vec<Node>,
    pub is_leader: bool,
    pub run_config: RunConfig,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P, this_id: u64) -> Self {
        let s = fs::read_to_string(&path).expect("read config");
        let docs = YamlLoader::load_from_str(&s).unwrap();
        let doc = &docs[0];

        let raw = doc["nodes"].as_vec().unwrap();

        // Build all nodes
        let nodes: Vec<Node> = raw
            .iter()
            .map(|e| {
                let id = e["node_id"].as_i64().unwrap() as u64;
                let pax = e["address"].as_str().unwrap().to_string();
                let role = match e["role"].as_str().unwrap() {
                    "proposer" => PaxosRole::Proposer,
                    "acceptor" => PaxosRole::Acceptor,
                    "learner" => PaxosRole::Learner,
                    "coordinator" => PaxosRole::Coordinator,
                    other => panic!("invalid role `{}`", other),
                };
                Node {
                    node_id: id,
                    address: pax,
                    role,
                }
            })
            .collect();

        // Determine leader = highest node_id
        let leader_id = nodes.iter().map(|n| n.node_id).max().unwrap();

        // Split out this node + remotes
        let is_leader = this_id == leader_id;
        let node = nodes
            .iter()
            .find(|n| n.node_id == this_id)
            .unwrap_or_else(|| panic!("node_id {} missing", this_id))
            .clone();
        let remote_nodes = nodes.into_iter().filter(|n| n.node_id != this_id).collect();

        // Parse run_config
        let r = &doc["run_config"];
        let run_config = RunConfig {
            is_event_driven: r["is_event_driven"].as_bool().unwrap(),
            acceptors_send_learns: r["acceptors_send_learns"].as_bool().unwrap(),
            learners_send_executed: r["learners_send_executed"].as_bool().unwrap(),
            prepare_timeout: r["prepare_timeout"].as_i64().unwrap() as u64,
            demo_client: r["demo_client"].as_bool().unwrap(),
            batch_size: r["batch_size"].as_i64().unwrap() as u64,
            tick_ms: r["tick_ms"].as_i64().unwrap() as u64,
            exec_interval_ms: r["exec_interval_ms"].as_i64().unwrap() as u64,
            retry_interval_ms: r["retry_interval_ms"].as_i64().unwrap() as u64,
            learn_max_gap: r["learn_max_gap"].as_i64().unwrap() as u64,
            executed_batch_size: r["executed_batch_size"].as_i64().unwrap() as u64,
            client_server_port: r["client_server_port"].as_i64().unwrap() as u16,
        };

        Config {
            node,
            remote_nodes,
            is_leader,
            run_config,
        }
    }
}
