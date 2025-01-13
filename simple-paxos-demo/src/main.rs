mod acceptor;
mod learner;
mod proposer;

use acceptor::Acceptor;
use learner::Learner;
use proposer::Proposer;

fn main() {
    // Initialize Paxos components
    let acceptors: Vec<Acceptor> = vec![Acceptor::new(), Acceptor::new(), Acceptor::new()];
    let mut proposer = Proposer::new();

    // Values to propose
    let values_to_propose = vec![
        "Value 1".to_string(),
        "Value 2".to_string(),
        "Value 3".to_string(),
        "Value 4".to_string(),
        "Value 5".to_string(),
    ];

    println!(
        "Starting Paxos proposal process with {} values...",
        values_to_propose.len()
    );

    for (index, value) in values_to_propose.iter().enumerate() {
        println!("INFO: Proposing value {}: {}", index + 1, value);

        // Propose the value to acceptors
        if proposer.propose(&acceptors, value.clone()) {
            println!("INFO: Proposal for '{}' accepted by a majority.", value);

            // Learner learns the value
            if let Some((proposal_id, learned_value)) = Learner::learn(&acceptors) {
                println!(
                    "INFO: Learner learned value from proposal ID {}: '{}'",
                    proposal_id, learned_value
                );
            } else {
                println!("WARNING: Learner failed to learn any value.");
            }
        } else {
            println!("WARNING: Proposal for '{}' was rejected.", value);
        }

        println!("--------------------------------------------");
    }

    println!("INFO: Paxos proposal process completed.");
}
