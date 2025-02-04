pub mod acceptor;
pub mod learner;
pub mod proposer;

pub trait AcceptorTrait {
    /// In the Prepare Phase the acceptor may promise not to accept proposals lower than `proposal_id`.
    fn prepare(&mut self, proposal_id: u64) -> bool;
    /// In the Accept Phase the acceptor may accept the proposal (if it has promised for this id).
    fn accept(&mut self, proposal_id: u64, value: String) -> bool;
    /// Returns the accepted proposal (if any).
    fn get_accepted_value(&self) -> Option<(u64, String)>;
}

pub trait ProposerTrait {
    /// Proposes a value using the given set of acceptors.
    fn propose(&mut self, acceptors: &mut [Box<dyn AcceptorTrait>], value: String) -> bool;
}

pub trait LearnerTrait {
    /// Learns the consensus value by checking the acceptors.
    fn learn(acceptors: &[Box<dyn AcceptorTrait>]) -> Option<(u64, String)>;
}
