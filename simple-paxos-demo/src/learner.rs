use crate::acceptor::Acceptor;

pub struct Learner;

impl Learner {
    pub fn learn(acceptors: &Vec<Acceptor>) -> Option<(u64, String)> {
        for acceptor in acceptors {
            if let Some(accepted) = &*acceptor.accepted_value.lock().unwrap() { // unwrap ignores errors
                return Some(accepted.clone());
            }
        }
        None
    }
}
