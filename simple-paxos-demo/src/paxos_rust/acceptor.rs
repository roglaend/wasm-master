use log::{info, warn};
use std::sync::Mutex;

#[derive(Default)]
pub struct Acceptor {
    pub highest_proposal_id: Mutex<u64>,
    pub accepted_value: Mutex<Option<(u64, String)>>, // (proposal_id, value)
}

impl Acceptor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn prepare(&self, proposal_id: u64) -> bool {
        let mut highest = self.highest_proposal_id.lock().unwrap();
        if proposal_id > *highest {
            info!("Acceptor: Preparing proposal ID {}", proposal_id);
            *highest = proposal_id;
            true
        } else {
            warn!(
                "Acceptor: Rejected prepare for proposal ID {}. Current highest: {}",
                proposal_id, *highest
            );
            false
        }
    }

    pub fn accept(&self, proposal_id: u64, value: String) -> bool {
        let mut highest = self.highest_proposal_id.lock().unwrap();
        if proposal_id >= *highest {
            info!(
                "Acceptor: Accepting proposal ID {} with value '{}'",
                proposal_id, value
            );
            *highest = proposal_id;
            let mut accepted = self.accepted_value.lock().unwrap();
            *accepted = Some((proposal_id, value));
            true
        } else {
            warn!(
                "Acceptor: Rejected accept for proposal ID {}. Current highest: {}",
                proposal_id, *highest
            );
            false
        }
    }
}
