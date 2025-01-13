#[cfg(feature = "rust")]
mod paxos_rust;
#[cfg(feature = "wasm")]
mod paxos_wasm;

use log::info;

fn main() {
    env_logger::init();

    #[cfg(feature = "rust")]
    {
        use paxos_rust::{acceptor::Acceptor, learner::Learner, proposer::Proposer};

        info!("Running Paxos using pure Rust implementation...");
        let acceptors = vec![Acceptor::new(), Acceptor::new(), Acceptor::new()];
        let mut proposer = Proposer::new();

        let values_to_propose = vec![
            "Value 1".to_string(),
            "Value 2".to_string(),
            "Value 3".to_string(),
        ];

        for value in values_to_propose {
            if proposer.propose(&acceptors, value.clone()) {
                if let Some((proposal_id, learned_value)) = Learner::learn(&acceptors) {
                    info!(
                        "Learner learned: Proposal ID: {}, Value: '{}'",
                        proposal_id, learned_value
                    );
                }
            }
        }
    }

    #[cfg(feature = "wasm")]
    {
        info!("Running Paxos using WASM components...");
        if let Err(e) = paxos_wasm::run_wasm_paxos() {
            eprintln!("Error running WASM Paxos: {}", e);
        }
    }
}
