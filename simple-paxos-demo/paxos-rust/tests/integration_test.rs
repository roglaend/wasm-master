use paxos_rust::paxos::acceptor::BasicAcceptor;
use paxos_rust::paxos::{learner::BasicLearner, proposer::BasicProposer, AcceptorTrait};
use paxos_rust::paxos::{LearnerTrait, ProposerTrait};
use paxos_rust::state_machine::{Command, CommandResult, KVStore};

/// Test that a single proposal reaches consensus and is learned.
#[test]
fn paxos_basic_test() {
    // Create a cluster of three acceptors.
    let mut acceptors: Vec<Box<dyn AcceptorTrait>> = vec![
        Box::new(BasicAcceptor::new()),
        Box::new(BasicAcceptor::new()),
        Box::new(BasicAcceptor::new()),
    ];
    let mut proposer = BasicProposer::new();

    let proposal_value = "Test Value".to_string();
    let success = proposer.propose(&mut acceptors, proposal_value.clone());
    assert!(success, "Proposal should be accepted by a majority");

    if let Some((proposal_id, learned_value)) = BasicLearner::learn(&acceptors) {
        assert_eq!(
            learned_value, proposal_value,
            "Learner should learn the correct value"
        );
        assert!(
            proposal_id > 0,
            "The learned proposal id should be positive (got {})",
            proposal_id
        );
    } else {
        panic!("Learner failed to reach consensus");
    }
}

/// Test that multiple sequential proposals are all accepted and learned correctly.
#[test]
fn paxos_multiple_proposals_test() {
    // Create a cluster of five acceptors.
    let mut acceptors: Vec<Box<dyn AcceptorTrait>> = (0..5)
        .map(|_| Box::new(BasicAcceptor::new()) as Box<dyn AcceptorTrait>)
        .collect();
    let mut proposer = BasicProposer::new();

    let proposals = vec![
        "Value 1".to_string(),
        "Value 2".to_string(),
        "Value 3".to_string(),
    ];

    for proposal in proposals {
        let success = proposer.propose(&mut acceptors, proposal.clone());
        assert!(
            success,
            "Proposal '{}' should be accepted by a majority",
            proposal
        );

        if let Some((_proposal_id, learned_value)) = BasicLearner::learn(&acceptors) {
            assert_eq!(
                learned_value, proposal,
                "Learner should learn the correct value for proposal '{}'",
                proposal
            );
        } else {
            panic!(
                "Learner failed to reach consensus for proposal '{}'",
                proposal
            );
        }
    }
}

/// Test the parsing of commands in the state machine.
#[test]
fn command_parsing_test() {
    // Valid commands.
    let valid_set = "SET key value";
    let valid_get = "GET key";

    // Parse a valid SET command.
    let cmd_set = Command::from_string(valid_set);
    assert!(
        cmd_set.is_ok(),
        "Expected SET command to parse successfully, got error: {:?}",
        cmd_set.err()
    );

    // Parse a valid GET command.
    let cmd_get = Command::from_string(valid_get);
    assert!(
        cmd_get.is_ok(),
        "Expected GET command to parse successfully, got error: {:?}",
        cmd_get.err()
    );

    // An invalid command should return an error.
    let invalid = "INVALID command";
    let cmd_invalid = Command::from_string(invalid);
    assert!(
        cmd_invalid.is_err(),
        "Expected invalid command to return an error"
    );
}

/// Test that the key-value state machine applies SET and GET commands correctly.
#[test]
fn state_machine_set_get_test() {
    let mut store = KVStore::new();

    // Test a SET command.
    let cmd_set =
        Command::from_string("SET mykey myvalue").expect("Failed to parse valid SET command");
    let result_set = store.apply(cmd_set);
    match result_set {
        CommandResult::Ack => { /* good */ }
        _ => panic!("Expected Ack result for SET command, got {:?}", result_set),
    }

    // Test a GET command.
    let cmd_get = Command::from_string("GET mykey").expect("Failed to parse valid GET command");
    let result_get = store.apply(cmd_get);
    match result_get {
        CommandResult::Value(val) => {
            assert_eq!(
                val,
                Some("myvalue".to_string()),
                "GET command returned incorrect value"
            );
        }
        _ => panic!(
            "Expected Value result for GET command, got {:?}",
            result_get
        ),
    }
}

/// A custom failing acceptor that always rejects proposals.
/// This will help simulate a scenario where a majority is not reached.
struct FailingAcceptor;

impl AcceptorTrait for FailingAcceptor {
    fn prepare(&mut self, _proposal_id: u64) -> bool {
        false // Always reject the prepare phase.
    }
    fn accept(&mut self, _proposal_id: u64, _value: String) -> bool {
        false // Always reject the accept phase.
    }
    fn get_accepted_value(&self) -> Option<(u64, String)> {
        None
    }
}

/// Test that a proposal fails when a majority is not reached.
/// We do this by mixing a failing acceptor into a small cluster.
#[test]
fn paxos_majority_failure_test() {
    // Create a cluster with two acceptors: one good and one failing.
    // With 2 acceptors, a majority is >1 (i.e. 2). With one failing, the proposal should fail.
    let mut acceptors: Vec<Box<dyn AcceptorTrait>> =
        vec![Box::new(BasicAcceptor::new()), Box::new(FailingAcceptor)];
    let mut proposer = BasicProposer::new();

    let proposal_value = "Test Failure".to_string();
    let success = proposer.propose(&mut acceptors, proposal_value);
    assert!(
        !success,
        "Proposal should fail when a majority of acceptors is not reached"
    );
}

/// Test that a proposal with an empty value is accepted correctly.
#[test]
fn paxos_empty_value_test() {
    // Create a cluster of three acceptors.
    let mut acceptors: Vec<Box<dyn AcceptorTrait>> = vec![
        Box::new(BasicAcceptor::new()),
        Box::new(BasicAcceptor::new()),
        Box::new(BasicAcceptor::new()),
    ];
    let mut proposer = BasicProposer::new();

    let proposal_value = "".to_string(); // empty string
    let success = proposer.propose(&mut acceptors, proposal_value.clone());
    assert!(success, "Empty proposal should be accepted by a majority");

    if let Some((_proposal_id, learned_value)) = BasicLearner::learn(&acceptors) {
        assert_eq!(
            learned_value, proposal_value,
            "Learner should learn the empty value correctly"
        );
    } else {
        panic!("Learner failed to reach consensus for empty proposal");
    }
}

/// Test that a proposal with a very large value is accepted correctly.
#[test]
fn paxos_large_value_test() {
    // Create a cluster of three acceptors.
    let mut acceptors: Vec<Box<dyn AcceptorTrait>> = vec![
        Box::new(BasicAcceptor::new()),
        Box::new(BasicAcceptor::new()),
        Box::new(BasicAcceptor::new()),
    ];
    let mut proposer = BasicProposer::new();

    // Create a large string (e.g., 10,000 characters).
    let proposal_value = "X".repeat(10_000);
    let success = proposer.propose(&mut acceptors, proposal_value.clone());
    assert!(success, "Large proposal should be accepted by a majority");

    if let Some((_proposal_id, learned_value)) = BasicLearner::learn(&acceptors) {
        assert_eq!(
            learned_value, proposal_value,
            "Learner should learn the large value correctly"
        );
    } else {
        panic!("Learner failed to reach consensus for large proposal");
    }
}

/// Test that a single acceptor (majority = 1) can reach consensus.
#[test]
fn paxos_single_acceptor_test() {
    // Create a cluster with only one acceptor.
    let mut acceptors: Vec<Box<dyn AcceptorTrait>> = vec![Box::new(BasicAcceptor::new())];
    let mut proposer = BasicProposer::new();

    let proposal_value = "Single".to_string();
    let success = proposer.propose(&mut acceptors, proposal_value.clone());
    assert!(
        success,
        "Proposal should succeed with a single acceptor (majority = 1)"
    );

    if let Some((_proposal_id, learned_value)) = BasicLearner::learn(&acceptors) {
        assert_eq!(
            learned_value, proposal_value,
            "Learner should learn the value for a single acceptor cluster"
        );
    } else {
        panic!("Learner failed to reach consensus with a single acceptor");
    }
}

/// Test that a proposal fails when no acceptors are available.
#[test]
fn paxos_no_acceptors_test() {
    // Create an empty cluster of acceptors.
    let mut acceptors: Vec<Box<dyn AcceptorTrait>> = vec![];
    let mut proposer = BasicProposer::new();

    let proposal_value = "No Acceptors".to_string();
    // Since there are no acceptors, the proposal should fail.
    let success = proposer.propose(&mut acceptors, proposal_value);
    assert!(
        !success,
        "Proposal should fail when there are no acceptors available"
    );
}

/// Test applying multiple commands to the state machine sequentially.
#[test]
fn state_machine_multiple_commands_test() {
    let mut store = KVStore::new();
    let commands = vec!["SET a 1", "SET b 2", "SET a 3", "GET a", "GET b"];

    for command in commands {
        let cmd = Command::from_string(command)
            .expect(&format!("Command '{}' should parse correctly", command));
        let _ = store.apply(cmd);
    }

    assert_eq!(
        store.store.get("a"),
        Some(&"3".to_string()),
        "State machine should have 'a' set to '3'"
    );
    assert_eq!(
        store.store.get("b"),
        Some(&"2".to_string()),
        "State machine should have 'b' set to '2'"
    );
}
