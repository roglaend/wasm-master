use std::{
    sync::{mpsc, Arc},
    thread,
};

use paxos_rust::network::simulation::{acceptor_node, proposer_node, PaxosMessage};

pub fn run() {
    println!("Running advanced Paxos simulation with network messaging...");

    // Create channels for each node.
    println!("Creating channels for nodes...");
    let (tx0, rx0) = mpsc::channel::<PaxosMessage>(); // Proposer node (ID 0)
    let (tx1, rx1) = mpsc::channel::<PaxosMessage>(); // Acceptor node 1
    let (tx2, rx2) = mpsc::channel::<PaxosMessage>(); // Acceptor node 2
    let (tx3, rx3) = mpsc::channel::<PaxosMessage>(); // Acceptor node 3

    // Build a network vector (each node’s sender is stored by index).
    println!("Building network vector with 4 nodes...");
    let network = Arc::new(vec![tx0, tx1, tx2, tx3]);

    // Spawn acceptor nodes (IDs 1, 2, 3).
    println!("Spawning acceptor nodes...");
    let net_for_node1 = Arc::clone(&network);
    let handle1 = thread::spawn(move || {
        println!("Starting acceptor node 1...");
        acceptor_node(1, rx1, net_for_node1);
        println!("Acceptor node 1 finished.");
    });
    let net_for_node2 = Arc::clone(&network);
    let handle2 = thread::spawn(move || {
        println!("Starting acceptor node 2...");
        acceptor_node(2, rx2, net_for_node2);
        println!("Acceptor node 2 finished.");
    });
    let net_for_node3 = Arc::clone(&network);
    let handle3 = thread::spawn(move || {
        println!("Starting acceptor node 3...");
        acceptor_node(3, rx3, net_for_node3);
        println!("Acceptor node 3 finished.");
    });

    // Spawn the proposer node (ID 0).
    println!("Spawning proposer node 0...");
    let net_for_proposer = Arc::clone(&network);
    let handle0 = thread::spawn(move || {
        println!("Starting proposer node 0...");
        proposer_node(rx0, net_for_proposer);
        println!("Proposer node 0 finished.");
    });

    // Wait for all nodes to finish.
    println!("Waiting for all nodes to finish...");
    handle0.join().expect("Proposer thread panicked");
    println!("Proposer node has terminated.");
    handle1.join().expect("Acceptor node 1 panicked");
    println!("Acceptor node 1 has terminated.");
    handle2.join().expect("Acceptor node 2 panicked");
    println!("Acceptor node 2 has terminated.");
    handle3.join().expect("Acceptor node 3 panicked");
    println!("Acceptor node 3 has terminated.");

    println!("Advanced Paxos simulation completed.");
}
