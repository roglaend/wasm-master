use log::warn;
use paxos::{acceptor::Acceptor, learner::Learner, proposer::Proposer};
use paxos_rust::paxos;

pub fn run() {
    env_logger::init();
    println!("Running Paxos using pure Rust...");

    // Initialize Acceptors (forming a cluster of 3)
    let mut acceptors = vec![Acceptor::new(), Acceptor::new(), Acceptor::new()];

    // Create a Proposer
    let mut proposer = Proposer::new();

    // List of values to propose
    let values_to_propose = vec![
        "Value 1".to_string(),
        "Value 2".to_string(),
        "Value 3".to_string(),
    ];

    for value in values_to_propose {
        println!("Proposing value: '{}'", value);

        // Run the Paxos proposal phase
        let success = proposer.propose(&mut acceptors, value.clone());

        if success {
            println!("Proposal '{}' was accepted by a majority!", value);
        } else {
            warn!("Proposal '{}' failed to reach consensus.", value);
            continue; // Skip learning if consensus was not reached
        }

        // The Learner attempts to determine consensus
        if let Some((proposal_id, learned_value)) = Learner::learn(&acceptors) {
            println!(
                "Learner confirmed consensus: Proposal ID: {}, Value: '{}'",
                proposal_id, learned_value
            );
        } else {
            warn!("Learner failed to reach consensus.");
        }
        println!("--------------------------------------");
    }
}
