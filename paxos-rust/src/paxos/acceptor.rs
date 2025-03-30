use log::{info, warn};

use crate::paxos::AcceptorTrait;

/// A basic implementation of a Paxos acceptor.
pub struct BasicAcceptor {
    promised_id: Option<u64>,
    accepted_id: Option<u64>,
    accepted_value: Option<String>,
}

impl BasicAcceptor {
    pub fn new() -> Self {
        BasicAcceptor {
            promised_id: None,
            accepted_id: None,
            accepted_value: None,
        }
    }
}

impl AcceptorTrait for BasicAcceptor {
    fn prepare(&mut self, proposal_id: u64) -> bool {
        // Grant a promise if we have not yet promised or if the new proposal id is higher.
        if self.promised_id.is_none() || proposal_id > self.promised_id.unwrap() {
            self.promised_id = Some(proposal_id);
            info!("Acceptor: Promised proposal ID {}", proposal_id);
            true
        } else {
            warn!(
                "Acceptor: Rejected prepare request for proposal ID {} (already promised ID {})",
                proposal_id,
                self.promised_id.unwrap()
            );
            false
        }
    }

    fn accept(&mut self, proposal_id: u64, value: String) -> bool {
        // Only accept if the proposal id is the one we last promised.
        if self.promised_id == Some(proposal_id) {
            self.accepted_id = Some(proposal_id);
            self.accepted_value = Some(value.clone());
            info!(
                "Acceptor: Accepted proposal ID {} with value '{}'",
                proposal_id, value
            );
            true
        } else {
            warn!(
                "Acceptor: Rejected accept request for proposal ID {} (promised ID: {:?})",
                proposal_id, self.promised_id
            );
            false
        }
    }

    fn get_accepted_value(&self) -> Option<(u64, String)> {
        if let (Some(id), Some(value)) = (self.accepted_id, self.accepted_value.clone()) {
            Some((id, value))
        } else {
            None
        }
    }

}
