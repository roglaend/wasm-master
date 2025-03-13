use crate::paxos_wasm::paxos_bindings::paxos::default::network::Node;

/// Shared configuration values for all nodes.
pub struct SharedConfig {
    // The leader is always node 1.
    pub leader_id: u64,
    // All endpoints in the cluster.
    pub endpoints: Vec<String>,
}

impl SharedConfig {
    pub fn load() -> Self {
        Self {
            leader_id: 1, // TODO: Make this dynamic
            // Define all endpoints here.
            endpoints: vec![
                "127.0.0.1:50051".to_string(),
                "127.0.0.1:50052".to_string(),
                // "127.0.0.1:50053".to_string(),
            ],
        }
    }
}

/// Complete configuration including both shared and node-specific values.
pub struct Config {
    pub node_id: u64,
    pub bind_addr: String,
    pub remote_nodes: Vec<Node>,
    pub leader_id: u64,
    pub _num_nodes: u64,
}

impl Config {
    /// Creates a full configuration based on the unique node_id.
    /// It computes the bind address for this node and constructs remote node instances for all other endpoints.
    pub fn new(node_id: u64) -> Self {
        let shared = SharedConfig::load();
        let num_nodes = shared.endpoints.len() as u64;

        // Validate that node_id is within the range of defined endpoints (using 1-indexing).
        if node_id == 0 || (node_id as usize) > shared.endpoints.len() {
            panic!(
                "Invalid node id. Must be between 1 and {}",
                shared.endpoints.len()
            );
        }

        // Our bind address is the endpoint corresponding to our node_id (1-indexed).
        let bind_addr = shared.endpoints[(node_id - 1) as usize].clone();

        // Create a Node for every endpoint that is not the bind address.
        let remote_nodes: Vec<Node> = shared
            .endpoints
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != (node_id as usize - 1))
            .map(|(i, ep)| Node {
                id: i as u64 + 1,
                address: ep.clone(),
            })
            .collect();

        Self {
            node_id,
            bind_addr,
            remote_nodes,
            leader_id: shared.leader_id,
            _num_nodes: num_nodes,
        }
    }
}
