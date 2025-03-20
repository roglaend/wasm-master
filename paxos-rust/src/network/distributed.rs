use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use log::{info, warn};

use crate::paxos::acceptor::BasicAcceptor;
use crate::paxos::AcceptorTrait;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PaxosMessage {
    Prepare {
        proposal_id: u64,
        from: usize,
    },
    Promise {
        proposal_id: u64,
        accepted_value: Option<String>,
        from: usize,
        ok: bool,
    },
    AcceptRequest {
        proposal_id: u64,
        value: String,
        from: usize,
    },
    Accepted {
        proposal_id: u64,
        value: String,
        from: usize,
        ok: bool,
    },
    Learn {
        proposal_id: u64,
        value: String,
    },
}

/// Send a message to a peer and wait for a response.
pub fn send_and_receive(
    peer_addr: &str,
    msg: &PaxosMessage,
) -> Result<PaxosMessage, Box<dyn Error>> {
    let stream = TcpStream::connect(peer_addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut writer = BufWriter::new(&stream);
    let json = serde_json::to_string(msg)?;
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    let mut reader = BufReader::new(&stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    let resp: PaxosMessage = serde_json::from_str(&response)?;
    Ok(resp)
}

/// Send a message to a peer without waiting for a response.
pub fn send_message(peer_addr: &str, msg: &PaxosMessage) -> Result<(), Box<dyn Error>> {
    let stream = TcpStream::connect(peer_addr)?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut writer = BufWriter::new(&stream);
    let json = serde_json::to_string(msg)?;
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

/// Global state for the node.
pub struct NodeState {
    pub node_id: usize,
    pub proposal_counter: u64,
    pub acceptor: BasicAcceptor,
}

impl NodeState {
    pub fn new(node_id: usize) -> Self {
        NodeState {
            node_id,
            proposal_counter: 0,
            acceptor: BasicAcceptor::new(),
        }
    }
}

/// Run the node server (node mode).  
/// Expected arguments (from the command line):
///   node <node_id> <address> <peer1> <peer2> ...
pub fn run_node(node_id: usize, address: &str, peers: Vec<String>) -> Result<(), Box<dyn Error>> {
    info!("Starting node {} on {}", node_id, address);
    let listener = TcpListener::bind(address)?;

    // Shared state for the node.
    let state = Arc::new(Mutex::new(NodeState::new(node_id)));
    let peers_arc = Arc::new(peers);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state_clone = Arc::clone(&state);
                let peers_clone = Arc::clone(&peers_arc);
                thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, state_clone, peers_clone) {
                        warn!("Error handling connection: {}", e);
                    }
                });
            }
            Err(e) => {
                warn!("Error accepting connection: {}", e);
            }
        }
    }
    Ok(())
}

/// Handle an incoming TCP connection.
fn handle_connection(
    stream: TcpStream,
    state: Arc<Mutex<NodeState>>,
    peers: Arc<Vec<String>>,
) -> Result<(), Box<dyn Error>> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    // Try to parse the line as JSON. If that fails, assume it’s a client command.
    if let Ok(msg) = serde_json::from_str::<PaxosMessage>(&line) {
        // It's a Paxos message from a peer.
        let response = process_paxos_message(msg, &state)?;
        let mut writer = BufWriter::new(&stream);
        let resp_json = serde_json::to_string(&response)?;
        writer.write_all(resp_json.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    } else {
        // Assume a client command. Expect format: "PROPOSE <command>"
        if line.starts_with("PROPOSE ") {
            let command = line["PROPOSE ".len()..].trim().to_string();
            info!(
                "Node {}: Received proposal command: {}",
                state.lock().unwrap().node_id,
                command
            );
            let success = run_proposal(command, &state, &peers)?;
            let mut writer = BufWriter::new(&stream);
            if success {
                writer.write_all(b"Proposal succeeded\n")?;
            } else {
                writer.write_all(b"Proposal failed\n")?;
            }
            writer.flush()?;
        }
    }
    Ok(())
}

/// Process an incoming PaxosMessage and generate a response.
fn process_paxos_message(
    msg: PaxosMessage,
    state: &Arc<Mutex<NodeState>>,
) -> Result<PaxosMessage, Box<dyn Error>> {
    let mut state = state.lock().unwrap();
    match msg {
        PaxosMessage::Prepare { proposal_id, from } => {
            let ok = state.acceptor.prepare(proposal_id);
            let resp = PaxosMessage::Promise {
                proposal_id,
                accepted_value: state.acceptor.get_accepted_value().map(|(_, v)| v),
                from: state.node_id,
                ok,
            };
            info!(
                "Node {}: Processed Prepare from node {}: {:?}",
                state.node_id, from, resp
            );
            Ok(resp)
        }
        PaxosMessage::AcceptRequest {
            proposal_id,
            value,
            from,
        } => {
            let ok = state.acceptor.accept(proposal_id, value.clone());
            let resp = PaxosMessage::Accepted {
                proposal_id,
                value,
                from: state.node_id,
                ok,
            };
            info!(
                "Node {}: Processed AcceptRequest from node {}: {:?}",
                state.node_id, from, resp
            );
            Ok(resp)
        }
        PaxosMessage::Learn { proposal_id, value } => {
            info!(
                "Node {}: Learned consensus for proposal {}: {}",
                state.node_id, proposal_id, value
            );
            Ok(PaxosMessage::Learn { proposal_id, value })
        }
        _ => {
            // For any other message type, return a default response.
            Ok(PaxosMessage::Learn {
                proposal_id: 0,
                value: "Unknown".to_string(),
            })
        }
    }
}

/// Run a Paxos proposal round as proposer (triggered by a client command).
fn run_proposal(
    command: String,
    state: &Arc<Mutex<NodeState>>,
    peers: &Arc<Vec<String>>,
) -> Result<bool, Box<dyn Error>> {
    {
        let mut state_guard = state.lock().unwrap();
        state_guard.proposal_counter += 1;
    }
    let proposal_id;
    let proposal_value = command;
    {
        proposal_id = state.lock().unwrap().proposal_counter;
    }

    info!(
        "Initiating Paxos proposal {} with value '{}'",
        proposal_id, proposal_value
    );

    let mut promise_count = 0;
    let majority = (peers.len() + 1) / 2 + 1; // count self and peers

    // Send Prepare to all peers.
    for peer in peers.iter() {
        if let Ok(resp) = send_and_receive(
            peer,
            &PaxosMessage::Prepare {
                proposal_id,
                from: 0,
            },
        ) {
            if let PaxosMessage::Promise { ok, .. } = resp {
                if ok {
                    promise_count += 1;
                }
            }
        }
    }
    // Process self.
    {
        let mut state_guard = state.lock().unwrap();
        if state_guard.acceptor.prepare(proposal_id) {
            promise_count += 1;
        }
    }
    info!(
        "Proposal {}: Received {} promises",
        proposal_id, promise_count
    );
    if promise_count < majority {
        warn!("Proposal {}: Not enough promises, aborting", proposal_id);
        return Ok(false);
    }

    // Send AcceptRequest to all peers.
    let mut accepted_count = 0;
    for peer in peers.iter() {
        if let Ok(resp) = send_and_receive(
            peer,
            &PaxosMessage::AcceptRequest {
                proposal_id,
                value: proposal_value.clone(),
                from: 0,
            },
        ) {
            if let PaxosMessage::Accepted { ok, .. } = resp {
                if ok {
                    accepted_count += 1;
                }
            }
        }
    }
    // Process self.
    {
        let mut state_guard = state.lock().unwrap();
        if state_guard
            .acceptor
            .accept(proposal_id, proposal_value.clone())
        {
            accepted_count += 1;
        }
    }
    info!(
        "Proposal {}: Received {} acceptances",
        proposal_id, accepted_count
    );
    if accepted_count < majority {
        warn!("Proposal {}: Not enough acceptances, aborting", proposal_id);
        return Ok(false);
    }

    // Broadcast Learn to all peers.
    for peer in peers.iter() {
        let _ = send_message(
            peer,
            &PaxosMessage::Learn {
                proposal_id,
                value: proposal_value.clone(),
            },
        );
    }
    {
        let mut state_guard = state.lock().unwrap();
        let _ = state_guard
            .acceptor
            .accept(proposal_id, proposal_value.clone());
    }
    info!("Proposal {}: Consensus reached", proposal_id);
    Ok(true)
}

/// Run client mode: Connect to a target node and send a proposal command.
/// Expected arguments: client <target_address> <command>
pub fn run_client(target_addr: &str, command: String) -> Result<(), Box<dyn Error>> {
    info!("Client: Connecting to {}", target_addr);
    let stream = TcpStream::connect(target_addr)?;
    let mut writer = BufWriter::new(&stream);
    let command_line = format!("PROPOSE {}\n", command);
    writer.write_all(command_line.as_bytes())?;
    writer.flush()?;
    let mut reader = BufReader::new(&stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    info!("Client: Received response: {}", response.trim());
    Ok(())
}
