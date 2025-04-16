use crate::bindings::paxos::default::{network_types::PaxosRole, paxos_types::Node};

pub struct SharedConfig {
    // The leader is by default node 1, but can be set dynamically.
    pub leader_id: u64,
    // All endpoints in the cluster.
    pub endpoints: Vec<Node>,
}

fn get_role_by_index(i: usize) -> PaxosRole {
    match i {
        0 | 1 => PaxosRole::Learner,      // first two
        2 | 3 | 4 => PaxosRole::Acceptor, // next three
        5 | 6 => PaxosRole::Proposer,     // last two
        _ => panic!("Index out of range for assigned roles!"),
    }
}

impl SharedConfig {
    pub fn load() -> Self {
        // Number of nodes to run. Defaults to 3 if NUM_NODES is not provided.
        let num_nodes: usize = std::env::var("NUM_NODES")
            .unwrap_or_else(|_| "7".to_string())
            .parse()
            .expect("NUM_NODES must be an integer");

        // Base port for the first node. Defaults to 50051.
        let base_port: u16 = std::env::var("BASE_PORT")
            .unwrap_or_else(|_| "50051".to_string())
            .parse()
            .expect("BASE_PORT must be a valid port number");

        // Base IP address. Defaults to 127.0.0.1.
        let base_ip = std::env::var("BASE_IP").unwrap_or_else(|_| "127.0.0.1".to_string());

        // Dynamically generate endpoints based on the number of nodes.
        let nodes: Vec<Node> = (0..num_nodes)
            .map(|i| {
                Node {
                    node_id: i as u64 + 1, // Node IDs are 1-indexed
                    address: format!("{}:{}", base_ip, base_port + i as u16),
                    role: get_role_by_index(i),
                }
            })
            .collect();

        // Leader ID. Defaults to 1 if not provided.
        let leader_id: u64 = std::env::var("LEADER_ID")
            .unwrap_or_else(|_| "1".to_string())
            .parse()
            .expect("LEADER_ID must be an integer");

        Self {
            leader_id,
            endpoints: nodes,
        }
    }
}

/// Complete configuration including both shared and node-specific values.
#[derive(Clone)]
pub struct Config {
    pub node: Node,
    pub bind_addr: String,
    pub remote_nodes: Vec<Node>,
    pub leader_id: u64,
    pub is_event_driven: bool,
}

impl Config {
    /// Creates a full configuration based on the unique node_id.
    /// It computes the bind address for this node and constructs remote node instances for all other endpoints.
    pub fn new(node_id: u64) -> Self {
        let shared = SharedConfig::load();

        // Validate that node_id is within the range of defined endpoints (using 1-indexing).
        if node_id == 0 || (node_id as usize) > shared.endpoints.len() {
            panic!(
                "Invalid node id. Must be between 1 and {}",
                shared.endpoints.len()
            );
        }

        // Our bind address is the endpoint corresponding to our node_id (1-indexed).
        let node_info = shared.endpoints[(node_id - 1) as usize].clone();

        // Build the Node for this current node.

        // Create a Node for every endpoint except for the current one.
        let remote_nodes: Vec<Node> = shared
            .endpoints
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != (node_id as usize - 1))
            .map(|(i, node_info)| Node {
                node_id: i as u64 + 1,
                address: node_info.address.clone(),
                role: node_info.clone().role,
            })
            .collect();

        // Read the event-driven flag from the environment.
        // Defaults to "false" if not provided.
        let is_event_driven: bool = std::env::var("IS_EVENT_DRIVEN")
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .expect("IS_EVENT_DRIVEN must be a boolean");

        Self {
            node: node_info.clone(),
            bind_addr: node_info.clone().address,
            remote_nodes,
            leader_id: shared.leader_id,
            is_event_driven,
        }
    }
}
