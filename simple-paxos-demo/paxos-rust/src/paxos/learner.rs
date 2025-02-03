use crate::paxos::{AcceptorTrait, LearnerTrait};
use log::{info, warn};
use std::collections::HashMap;

/// A basic Paxos learner.
pub struct BasicLearner;

impl BasicLearner {
    pub fn new() -> Self {
        BasicLearner
    }
}

impl LearnerTrait for BasicLearner {
    fn learn(acceptors: &[Box<dyn AcceptorTrait>]) -> Option<(u64, String)> {
        let mut accepted_values = vec![];

        // Collect accepted proposals from all acceptors.
        for acceptor in acceptors {
            if let Some(accepted) = acceptor.get_accepted_value() {
                accepted_values.push(accepted);
            }
        }

        if accepted_values.is_empty() {
            warn!("Learner: No values accepted by any acceptor.");
            return None;
        }

        // Tally the accepted values.
        let mut count_map: HashMap<String, (usize, u64)> = HashMap::new();
        for (id, value) in accepted_values {
            let entry = count_map.entry(value.clone()).or_insert((0, id));
            entry.0 += 1;
            if entry.0 > (acceptors.len() / 2) {
                info!(
                    "Learner: Reached consensus on proposal ID {} with value '{}'",
                    entry.1, value
                );
                return Some((entry.1, value));
            }
        }

        warn!("Learner: No consensus reached.");
        None
    }
}
