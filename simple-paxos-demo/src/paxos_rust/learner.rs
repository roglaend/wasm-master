use crate::paxos_rust::acceptor::Acceptor;
use log::info;

pub struct Learner;

impl Learner {
    pub fn learn(acceptors: &Vec<Acceptor>) -> Option<(u64, String)> {
        for acceptor in acceptors {
            if let Some(accepted) = &*acceptor.accepted_value.lock().unwrap() {
                info!(
                    "Learner: Learned value '{}' from proposal ID {}",
                    accepted.1, accepted.0
                );
                return Some(accepted.clone());
            }
        }
        return None;
    }
}
