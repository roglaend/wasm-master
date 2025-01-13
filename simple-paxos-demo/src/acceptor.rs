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
        let mut highest = self.highest_proposal_id.lock().unwrap(); // unwrap ignores errors
        if proposal_id > *highest {
            *highest = proposal_id;
            true
        } else {
            false
        }
    }

    pub fn accept(&self, proposal_id: u64, value: String) -> bool {
        let mut highest = self.highest_proposal_id.lock().unwrap(); // unwrap ignores errors
        if proposal_id >= *highest {
            *highest = proposal_id;
            let mut accepted = self.accepted_value.lock().unwrap(); // unwrap ignores errors
            *accepted = Some((proposal_id, value));
            true
        } else {
            false
        }
    }
}
