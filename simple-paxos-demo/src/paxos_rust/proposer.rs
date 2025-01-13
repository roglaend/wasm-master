use crate::paxos_rust::acceptor::Acceptor;
use log::{info, warn};

pub struct Proposer {
    pub proposal_id: u64,
}

impl Proposer {
    pub fn new() -> Self {
        Self { proposal_id: 0 }
    }

    pub fn propose(&mut self, acceptors: &Vec<Acceptor>, value: String) -> bool {
        self.proposal_id += 1;
        info!(
            "Proposer: Proposing value '{}' with proposal ID {}",
            value, self.proposal_id
        );

        let mut successful = 0;
        for acceptor in acceptors {
            if acceptor.prepare(self.proposal_id)
                && acceptor.accept(self.proposal_id, value.clone())
            {
                successful += 1;
            }
        }

        let quorum = acceptors.len() / 2;
        if successful > quorum {
            info!(
                "Proposer: Proposal '{}' accepted by quorum. Successful: {}, Required: {}",
                value, successful, quorum
            );
            true
        } else {
            warn!(
                "Proposer: Proposal '{}' rejected. Successful: {}, Required: {}",
                value, successful, quorum
            );
            false
        }
    }
}
