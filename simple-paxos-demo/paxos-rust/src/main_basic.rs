// Import Paxos roles and the state machine.
use paxos::acceptor::BasicAcceptor;
use paxos::learner::BasicLearner;
use paxos::proposer::BasicProposer;
use paxos_rust::{
    paxos::{self, AcceptorTrait, LearnerTrait, ProposerTrait},
    state_machine::{Command, CommandResult, KVStore},
};

pub fn run() {
    println!("Running Paxos using pure Rust with a key-value state machine...");

    // Create a cluster of 3 acceptors. We store them as Box<dyn AcceptorTrait>
    let mut acceptors: Vec<Box<dyn AcceptorTrait>> = vec![
        Box::new(BasicAcceptor::new()),
        Box::new(BasicAcceptor::new()),
        Box::new(BasicAcceptor::new()),
    ];

    // Create a proposer.
    let mut proposer = BasicProposer::new();

    // Create a simple key-value state machine.
    let mut kv_store = KVStore::new();

    // List of commands (as strings) to propose.
    // (For a replicated state machine, these would be the operations that need to be agreed upon.)
    let commands = vec![
        "SET key1 value1".to_string(),
        "SET key2 value2".to_string(),
        "GET key1".to_string(),
    ];

    for command_str in commands {
        println!("Proposing command: '{}'", command_str);

        // Run the Paxos proposal phase.
        let success = proposer.propose(&mut acceptors, command_str.clone());

        if success {
            println!("Proposal '{}' was accepted by a majority!", command_str);
        } else {
            println!("Proposal '{}' failed to reach consensus.", command_str);
            continue; // Skip learning if consensus was not reached.
        }

        // The learner attempts to determine consensus.
        if let Some((_proposal_id, learned_command)) = BasicLearner::learn(&acceptors) {
            println!("Learner confirmed consensus: {}", learned_command);

            // Try to parse the learned command and apply it to the state machine.
            match Command::from_string(&learned_command) {
                Ok(cmd) => {
                    let result = kv_store.apply(cmd);
                    match result {
                        CommandResult::Ack => {
                            println!("State machine applied command successfully.");
                        }
                        CommandResult::Value(val) => {
                            println!("State machine returned value: {:?}", val);
                        }
                        CommandResult::Error(err) => {
                            println!("State machine error: {}", err);
                        }
                    }
                }
                Err(e) => {
                    println!("Failed to parse command: {}", e);
                }
            }
        } else {
            println!("Learner failed to reach consensus.");
        }
        println!("--------------------------------------");
    }

    // After processing all commands, print the final state of the key-value store.
    println!("Final state of key-value store:");
    for (key, value) in kv_store.store.iter() {
        println!("{}: {}", key, value);
    }
}
