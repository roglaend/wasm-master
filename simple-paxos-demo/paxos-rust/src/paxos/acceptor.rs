use log::{info, warn};

pub struct Acceptor {
    promised_id: Option<u64>,
    accepted_id: Option<u64>,
    accepted_value: Option<String>,
}

impl Acceptor {
    pub fn new() -> Self {
        Acceptor {
            promised_id: None,
            accepted_id: None,
            accepted_value: None,
        }
    }

    pub fn prepare(&mut self, proposal_id: u64) -> bool {
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

    pub fn accept(&mut self, proposal_id: u64, value: String) -> bool {
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

    pub fn get_accepted_value(&self) -> Option<(u64, String)> {
        if let (Some(id), Some(value)) = (self.accepted_id, self.accepted_value.clone()) {
            Some((id, value))
        } else {
            None
        }
    }
}
