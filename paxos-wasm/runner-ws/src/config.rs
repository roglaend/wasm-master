use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};
use yaml_rust::YamlLoader;

use crate::bindings::paxos::default::{
    logger::Level,
    paxos_types::{Node, PaxosRole, RunConfig},
};

#[derive(Clone)]
pub struct Config {
    /// every node that appears in *any* cluster
    pub active_nodes: Vec<Node>,

    /// the subset that belongs to this cluster
    pub cluster_nodes: Vec<Node>,

    /// highest node_id across all *active* nodes
    pub leader_id: u64,

    pub run_config: RunConfig,
    pub log_level: Level,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P, cluster_id: u64) -> anyhow::Result<Self> {
        let s = fs::read_to_string(path)?;
        let docs = YamlLoader::load_from_str(&s)?;
        let doc = &docs[0];

        // parse log level
        let log_level = match doc["log_level"].as_str().unwrap_or("info") {
            "debug" => Level::Debug,
            "info" => Level::Info,
            "warning" => Level::Warn,
            "error" => Level::Error,
            o => anyhow::bail!("bad log_level `{}`", o),
        };

        // parse all nodes
        let all_nodes: Vec<Node> = doc["nodes"]
            .as_vec()
            .unwrap()
            .iter()
            .map(|n| {
                let id = n["node_id"].as_i64().unwrap() as u64;
                let addr = n["address"].as_str().unwrap().to_string();
                let role = match n["role"].as_str().unwrap() {
                    "proposer" => PaxosRole::Proposer,
                    "acceptor" => PaxosRole::Acceptor,
                    "learner" => PaxosRole::Learner,
                    "coordinator" => PaxosRole::Coordinator,
                    other => panic!("bad role `{}`", other),
                };
                Node {
                    node_id: id,
                    address: addr,
                    role,
                }
            })
            .collect();

        // parse clusters → map<cluster_id, Vec<node_id>>
        let mut clusters = HashMap::new();
        for (k, y) in doc["clusters"].as_hash().unwrap().iter() {
            let cluster_id = k.as_i64().unwrap() as u64;
            let node_ids: Vec<u64> = y
                .as_vec()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as u64)
                .collect();
            clusters.insert(cluster_id, node_ids);
        }

        // Build the set of *all* active node IDs (anyone in any cluster)
        let mut active_id_set = HashSet::new();
        for ids in clusters.values() {
            active_id_set.extend(ids.iter().copied());
        }
        // Now filter your full node list by that set
        let active_nodes: Vec<_> = all_nodes
            .iter()
            .filter(|n| active_id_set.contains(&n.node_id))
            .cloned()
            .collect();

        // Pick the leader among *active_nodes*
        let leader_id = active_nodes
            .iter()
            .map(|n| n.node_id)
            .max()
            .expect("must have at least one active node");

        // Now lookup *this* cluster’s node IDs
        let this_cluster_ids = clusters
            .get(&cluster_id)
            .ok_or_else(|| anyhow::anyhow!("no cluster {} in config", cluster_id))?;

        // And filter active_nodes again for just *this* cluster
        let cluster_nodes: Vec<_> = active_nodes
            .iter()
            .filter(|n| this_cluster_ids.contains(&n.node_id))
            .cloned()
            .collect();

        // parse run_config block
        let r = &doc["run_config"];
        let crashes = r["crashes"]
            .as_vec()
            .unwrap_or(&vec![])
            .iter()
            .map(|entry| {
                entry
                    .as_vec()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_i64().unwrap() as u64)
                    .collect::<Vec<u64>>()
            })
            .collect::<Vec<Vec<u64>>>();
        let run_config = RunConfig {
            is_event_driven: r["is_event_driven"].as_bool().unwrap(),
            acceptors_send_learns: r["acceptors_send_learns"].as_bool().unwrap(),
            learners_send_executed: r["learners_send_executed"].as_bool().unwrap(),
            prepare_timeout: r["prepare_timeout"].as_i64().unwrap() as u64,
            demo_client: r["demo_client"].as_bool().unwrap(),
            demo_client_requests: r["demo_client_requests"].as_i64().unwrap() as u64,
            batch_size: r["batch_size"].as_i64().unwrap() as u64,
            tick_micros: r["tick_micros"].as_i64().unwrap() as u64,
            exec_interval_ms: r["exec_interval_ms"].as_i64().unwrap() as u64,
            retry_interval_ms: r["retry_interval_ms"].as_i64().unwrap() as u64,
            learn_max_gap: r["learn_max_gap"].as_i64().unwrap() as u64,
            message_batch_size: r["message_batch_size"].as_i64().unwrap() as u64,
            client_server_port: r["client_server_port"].as_i64().unwrap() as u16,
            persistent_storage: r["persistent_storage"].as_bool().unwrap(),
            heartbeats: r["heartbeats"].as_bool().unwrap(),
            heartbeat_interval_ms: r["heartbeat_interval_ms"].as_i64().unwrap() as u64,
            crashes,
            storage_load_snapshots: r["storage_load_snapshots"].as_i64().unwrap() as u64,
            storage_max_snapshots: r["storage_max_snapshots"].as_i64().unwrap() as u64,
            storage_flush_state_count: r["storage_flush_state_count"].as_i64().unwrap() as u64,
            storage_flush_state_interval_ms: r["storage_flush_state_interval_ms"].as_i64().unwrap()
                as u64,
            storage_flush_change_count: r["storage_flush_change_count"].as_i64().unwrap() as u64,
            storage_flush_change_interval_ms: r["storage_flush_change_interval_ms"]
                .as_i64()
                .unwrap() as u64,
            storage_snapshot_slot_interval: r["storage_snapshot_slot_interval"].as_i64().unwrap()
                as u64,
        };

        Ok(Config {
            active_nodes,
            cluster_nodes,
            leader_id,
            run_config,
            log_level,
        })
    }
}
