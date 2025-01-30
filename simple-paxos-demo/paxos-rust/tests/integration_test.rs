use paxos_rust::paxos::{acceptor::Acceptor, learner::Learner, proposer::Proposer};

#[test]
fn paxos_basic_test() {
    let mut acceptors = vec![Acceptor::new(), Acceptor::new(), Acceptor::new()];
    let mut proposer = Proposer::new();

    let proposal_value = "Test Value".to_string();
    let success = proposer.propose(&mut acceptors, proposal_value.clone());

    assert!(success, "Proposal should be accepted");

    if let Some((_, learned_value)) = Learner::learn(&acceptors) {
        assert_eq!(
            learned_value, proposal_value,
            "Learner should learn the correct value"
        );
    } else {
        panic!("Learner failed to reach consensus");
    }
}
