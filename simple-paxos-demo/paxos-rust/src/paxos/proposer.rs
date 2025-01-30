use crate::paxos::acceptor::Acceptor;
use log::{info, warn};

pub struct Proposer {
    proposal_id: u64,
}

impl Proposer {
    pub fn new() -> Self {
        Proposer { proposal_id: 0 }
    }

    pub fn propose(&mut self, acceptors: &mut [Acceptor], value: String) -> bool {
        self.proposal_id += 1;
        info!(
            "Proposer: Initiating proposal ID {} with value '{}'",
            self.proposal_id, value
        );

        // Prepare Phase
        let mut prepare_success_count = 0;
        for acceptor in acceptors.iter_mut() {
            if acceptor.prepare(self.proposal_id) {
                prepare_success_count += 1;
            }
        }
        info!(
            "Proposer: {} out of {} acceptors responded positively to prepare phase",
            prepare_success_count,
            acceptors.len()
        );

        // Ensure quorum for accept phase
        if prepare_success_count > acceptors.len() / 2 {
            let mut accept_success_count = 0;
            for acceptor in acceptors.iter_mut() {
                if acceptor.accept(self.proposal_id, value.clone()) {
                    accept_success_count += 1;
                }
            }
            info!(
                "Proposer: {} out of {} acceptors accepted the proposal",
                accept_success_count,
                acceptors.len()
            );

            return accept_success_count > acceptors.len() / 2;
        }

        warn!(
            "Proposer: Proposal ID {} failed to reach a majority",
            self.proposal_id
        );
        false
    }
}
