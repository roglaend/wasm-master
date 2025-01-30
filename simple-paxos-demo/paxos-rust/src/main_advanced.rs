use log::warn;
use paxos::{acceptor::Acceptor, learner::Learner, proposer::Proposer};
use paxos_rust::paxos;
use rand::Rng;
use std::{thread, time::Duration};

pub fn run() {
    env_logger::init();
    println!("Running Paxos using pure Rust...");

    let mut rng = rand::rng();

    // Simulating a distributed network of 5 acceptors
    let mut acceptors = vec![
        Acceptor::new(),
        Acceptor::new(),
        Acceptor::new(),
        Acceptor::new(),
        Acceptor::new(),
    ];

    // Multiple proposers (simulating independent clients making proposals)
    let mut proposers = vec![Proposer::new(), Proposer::new()];

    // Values proposed from different "clients"
    let values_to_propose = vec![
        "Value A".to_string(),
        "Value B".to_string(),
        "Value C".to_string(),
        "Value D".to_string(),
    ];

    // Tracking learned values
    let mut learned_values = vec![];

    for value in values_to_propose {
        let proposer_index = rng.random_range(0..proposers.len()); // Random proposer selection
        let proposer = &mut proposers[proposer_index];

        println!(
            "Proposer {} is proposing value: '{}'",
            proposer_index, value
        );

        // Simulating network delay
        let delay = rng.random_range(100..500);
        thread::sleep(Duration::from_millis(delay));

        // Introduce artificial failure probability (30% chance of failure)
        if rng.random_bool(0.3) {
            warn!(
                "Proposer {} encountered network failure! Proposal '{}' aborted.",
                proposer_index, value
            );
            continue; // Skip this proposal
        }

        let success = proposer.propose(&mut acceptors, value.clone());

        if success {
            println!("Proposal '{}' was accepted by a majority!", value);
        } else {
            warn!("Proposal '{}' failed to reach consensus.", value);
            continue;
        }

        // The Learner attempts to determine the latest consensus value
        if let Some((proposal_id, learned_value)) = Learner::learn(&acceptors) {
            println!(
                "Learner confirmed consensus: Proposal ID: {}, Value: '{}'",
                proposal_id, learned_value
            );
            learned_values.push(learned_value);
        } else {
            warn!("Learner failed to reach consensus.");
        }

        println!("--------------------------------------");
    }

    // Final summary of learned values
    println!("Final Learned Values: {:?}", learned_values);
}
