use log::{info, warn};
use rand::Rng;
use std::{
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};

use crate::paxos::{acceptor::BasicAcceptor, AcceptorTrait};

#[derive(Debug, Clone)]
pub enum PaxosMessage {
    // Phase 1: A proposer asks an acceptor to prepare for a proposal.
    Prepare {
        proposal_id: u64,
        from: usize, // sender's node id
    },
    // Response from an acceptor to a Prepare.
    Promise {
        proposal_id: u64,
        accepted_value: Option<String>,
        from: usize, // sender's node id
        ok: bool,    // true if the acceptor accepted the prepare
    },
    // Phase 2: The proposer sends a value to be accepted.
    AcceptRequest {
        proposal_id: u64,
        value: String,
        from: usize,
    },
    // Response from an acceptor to an AcceptRequest.
    Accepted {
        proposal_id: u64,
        value: String,
        from: usize,
        ok: bool, // true if the acceptor accepted the proposal
    },
    // Phase 3: The chosen value is broadcast to all nodes.
    Learn {
        proposal_id: u64,
        value: String,
    },
}

/// Simulate sending a message over the network with a random delay.
pub fn send_message(
    network: &Arc<Vec<mpsc::Sender<PaxosMessage>>>,
    dest: usize,
    msg: PaxosMessage,
) {
    let delay = rand::rng().random_range(10..100);
    thread::sleep(Duration::from_millis(delay));
    network[dest]
        .send(msg)
        .unwrap_or_else(|e| panic!("Failed to send message to node {}: {}", dest, e));
}

/// Acceptor node: waits for incoming Paxos messages and responds accordingly.
pub fn acceptor_node(
    id: usize,
    rx: mpsc::Receiver<PaxosMessage>,
    network: Arc<Vec<mpsc::Sender<PaxosMessage>>>,
) {
    info!("Acceptor node {} started.", id);
    let mut acceptor = BasicAcceptor::new();

    loop {
        if let Ok(msg) = rx.recv() {
            match msg {
                PaxosMessage::Prepare { proposal_id, from } => {
                    let ok = acceptor.prepare(proposal_id);
                    let promise_msg = PaxosMessage::Promise {
                        proposal_id,
                        accepted_value: acceptor.get_accepted_value().map(|(_, v)| v),
                        from: id,
                        ok,
                    };
                    info!(
                        "Node {}: Received Prepare({}) from node {}. Sending Promise: {:?}",
                        id, proposal_id, from, promise_msg
                    );
                    send_message(&network, from, promise_msg);
                }
                PaxosMessage::AcceptRequest {
                    proposal_id,
                    value,
                    from,
                } => {
                    let ok = acceptor.accept(proposal_id, value.clone());
                    let accepted_msg = PaxosMessage::Accepted {
                        proposal_id,
                        value: value.clone(),
                        from: id,
                        ok,
                    };
                    info!(
                        "Node {}: Received AcceptRequest({}) from node {}. Sending Accepted: {:?}",
                        id, proposal_id, from, accepted_msg
                    );
                    send_message(&network, from, accepted_msg);
                }
                PaxosMessage::Learn { proposal_id, value } => {
                    info!(
                        "Node {}: Learned consensus (proposal {}): '{}'",
                        id, proposal_id, value
                    );
                    break; // Exit after learning the consensus value.
                }
                _ => {
                    // Ignore other message types.
                }
            }
        } else {
            break;
        }
    }
    info!("Acceptor node {} exiting.", id);
}

/// Proposer node: executes Paxos phases to propose a value.
pub fn proposer_node(
    rx: mpsc::Receiver<PaxosMessage>,
    network: Arc<Vec<mpsc::Sender<PaxosMessage>>>,
) {
    info!("Proposer node started.");
    let proposal_id = 1;
    let proposal_value = "Distributed Value".to_string();
    let num_acceptors = network.len() - 1; // node 0 is the proposer.
    let majority = num_acceptors / 2 + 1;

    // --- Phase 1: Prepare ---
    info!(
        "Proposer: Sending Prepare messages for proposal {}.",
        proposal_id
    );
    for dest in 1..network.len() {
        let msg = PaxosMessage::Prepare {
            proposal_id,
            from: 0,
        };
        send_message(&network, dest, msg);
    }

    let mut promise_count = 0;
    let mut responses_received = 0;
    while responses_received < num_acceptors {
        if let Ok(msg) = rx.recv_timeout(Duration::from_secs(2)) {
            if let PaxosMessage::Promise {
                proposal_id: _,
                accepted_value: _,
                from,
                ok,
            } = msg
            {
                info!("Proposer: Received Promise from node {}: ok={}", from, ok);
                responses_received += 1;
                if ok {
                    promise_count += 1;
                }
            }
        } else {
            warn!("Proposer: Timeout waiting for Promise responses.");
            break;
        }
    }
    if promise_count < majority {
        warn!(
            "Proposer: Only {} promises received (needed {}). Aborting proposal.",
            promise_count, majority
        );
        return;
    }
    info!("Proposer: Majority promises received. Moving to Accept phase.");

    // --- Phase 2: Accept Request ---
    for dest in 1..network.len() {
        let msg = PaxosMessage::AcceptRequest {
            proposal_id,
            value: proposal_value.clone(),
            from: 0,
        };
        send_message(&network, dest, msg);
    }
    let mut accepted_count = 0;
    responses_received = 0;
    while responses_received < num_acceptors {
        if let Ok(msg) = rx.recv_timeout(Duration::from_secs(2)) {
            if let PaxosMessage::Accepted {
                proposal_id: _,
                value: _,
                from,
                ok,
            } = msg
            {
                info!("Proposer: Received Accepted from node {}: ok={}", from, ok);
                responses_received += 1;
                if ok {
                    accepted_count += 1;
                }
            }
        } else {
            warn!("Proposer: Timeout waiting for Accepted responses.");
            break;
        }
    }
    if accepted_count < majority {
        warn!(
            "Proposer: Only {} acceptances received (needed {}). Aborting proposal.",
            accepted_count, majority
        );
        return;
    }
    info!("Proposer: Proposal accepted by majority. Broadcasting Learn message.");

    // --- Phase 3: Learn ---
    for dest in 0..network.len() {
        let msg = PaxosMessage::Learn {
            proposal_id,
            value: proposal_value.clone(),
        };
        send_message(&network, dest, msg);
    }
    info!("Proposer: Consensus reached and Learn message broadcasted. Exiting.");
}
