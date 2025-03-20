use log::{info, warn};

use crate::paxos::{AcceptorTrait, ProposerTrait};

/// A basic Paxos proposer.
pub struct BasicProposer {
    proposal_id: u64,
}

impl BasicProposer {
    pub fn new() -> Self {
        BasicProposer { proposal_id: 0 }
    }
}

impl ProposerTrait for BasicProposer {
    fn propose(&mut self, acceptors: &mut [Box<dyn AcceptorTrait>], value: String) -> bool {
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

        // Require a quorum for the Accept Phase.
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
