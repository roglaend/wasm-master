use crate::paxos::acceptor::Acceptor;
use log::{info, warn};

pub struct Learner;

impl Learner {
    pub fn learn(acceptors: &[Acceptor]) -> Option<(u64, String)> {
        let mut accepted_values = vec![];

        for acceptor in acceptors {
            if let Some(accepted) = acceptor.get_accepted_value() {
                accepted_values.push(accepted);
            }
        }

        if accepted_values.is_empty() {
            warn!("Learner: No values accepted by any acceptor.");
            return None;
        }

        let mut consensus_value = None;
        let mut highest_id = 0;
        let mut count_map = std::collections::HashMap::new();

        for (id, value) in accepted_values {
            let count = count_map.entry(value.clone()).or_insert(0);
            *count += 1;

            if *count > acceptors.len() / 2 {
                consensus_value = Some((id, value.clone()));
                highest_id = id;
            }
        }

        if let Some(value) = consensus_value {
            info!(
                "Learner: Reached consensus on proposal ID {} with value '{}'",
                highest_id, value.1
            );
            return Some((highest_id, value.1));
        }

        warn!("Learner: No consensus reached.");
        None
    }
}
