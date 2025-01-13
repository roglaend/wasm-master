use crate::acceptor::Acceptor;

pub struct Proposer {
    pub proposal_id: u64,
}

impl Proposer {
    pub fn new() -> Self {
        Self { proposal_id: 0 }
    }

    pub fn propose(&mut self, acceptors: &Vec<Acceptor>, value: String) -> bool {
        self.proposal_id += 1;
        let mut successful = 0;

        for acceptor in acceptors {
            if acceptor.prepare(self.proposal_id)
                && acceptor.accept(self.proposal_id, value.clone())
            {
                successful += 1;
            }
        }

        // Quorum: Simple majority
        successful > acceptors.len() / 2
    }
}
