pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "learner-agent-world",
        // generate_unused_types: true,
        additional_derives: [PartialEq, Clone],
    });
}

bindings::export!(MyLearnerAgent with_types_in bindings);

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bindings::exports::paxos::default::learner_agent::{
    Guest as GuestLearnerAgent, GuestLearnerAgentResource,
};

use bindings::paxos::default::learner_types::{LearnResult, LearnResultTest};

use bindings::paxos::default::learner::LearnerResource;

use bindings::paxos::default::{logger, network};

use bindings::paxos::default::network_types::{Heartbeat, MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{
    ClientResponse, Node, PaxosRole, RunConfig, Slot, Value,
};

pub struct MyLearnerAgent;

impl GuestLearnerAgent for MyLearnerAgent {
    type LearnerAgentResource = MyLearnerAgentResource;
}

pub struct MyLearnerAgentResource {
    config: RunConfig,

    node: Node,

    learner: Arc<LearnerResource>,
    proposers: Vec<Node>,
    acceptors: Vec<Node>,

    simulated_state_machine: RefCell<HashMap<String, String>>,
    retires: RefCell<HashMap<Slot, Instant>>,
}

impl MyLearnerAgentResource {
    fn possible_retry(&self, slot: Slot, retry_interval: Duration) -> bool {
        match self.retires.borrow().get(&slot) {
            Some(&last_retry_time) => {
                // If not enough time has passed since last retry, do nothing
                if last_retry_time.elapsed() < retry_interval {
                    return false;
                }
            }
            None => {
                // If we have no entry, then it's the first time we retry
            }
        }

        // If we reach here, we should do a retry
        // Update the timestamp to now and return true to indicate a retry happened
        self.retires.borrow_mut().insert(slot, Instant::now());
        true
    }

    // simulates getter and setter of a state machine :clownemoji:
    fn execute_command(&self, val: Value) -> String {
        if let Some(cmd) = val.command {
            let string_split = cmd.split(" ").collect::<Vec<_>>();
            if string_split.len() > 1 {
                let command = string_split[0];
                let key = string_split[1];
                match command {
                    "set" => {
                        let value = string_split[2];
                        self.simulated_state_machine
                            .borrow_mut()
                            .insert(key.to_string(), value.to_string());
                        return format!("Set {} to {}", key, value);
                    }
                    "get" => {
                        if let Some(value) = self.simulated_state_machine.borrow().get(key) {
                            return format!("{} is {}", key, value);
                        } else {
                            return format!("{} not found", key);
                        }
                    }
                    _ => {
                        return format!("Unknown command: {}", command);
                    }
                }
            } else {
                return "Invalid command".to_string();
            }
        }

        // If no command is found, return a default message
        "Not command found".to_string()
    }
}

impl GuestLearnerAgentResource for MyLearnerAgentResource {
    fn new(node: Node, nodes: Vec<Node>, config: RunConfig) -> Self {
        let learner = Arc::new(LearnerResource::new());

        let proposers: Vec<_> = nodes
            .clone()
            .into_iter()
            .filter(|x| x.role == PaxosRole::Proposer || x.role == PaxosRole::Coordinator)
            .collect();

        let acceptors: Vec<_> = nodes
            .clone()
            .into_iter()
            .filter(|x| x.role == PaxosRole::Acceptor || x.role == PaxosRole::Coordinator)
            .collect();

        logger::log_info("[Acceptor Agent] Initialized core acceptor resource.");
        Self {
            config,
            node,
            learner,
            proposers,
            acceptors,
            retires: RefCell::new(HashMap::new()),
            simulated_state_machine: RefCell::new(HashMap::new()),
        }
    }

    fn get_learned_value(&self) -> Option<String> {
        todo!()
    }

    // TODO : fix proto for this so that it can actually be used
    fn get_state(&self) -> bindings::paxos::default::learner_types::LearnerState {
        self.learner.get_state()
    }

    // Ticker called from host at a slower interval than proposer ticker
    fn run_paxos_loop(&self) -> () {
        if let Some(missing_slot) = self.learner.check_for_gap() {
            let retry_interval = Duration::from_millis(500);
            match self.possible_retry(missing_slot, retry_interval) {
                true => {
                    let retry = NetworkMessage {
                        sender: self.node.clone(),
                        payload: MessagePayload::RetryLearn(missing_slot),
                    };
                    logger::log_warn(&format!(
                        "[Learner Agent] Retrying LEARN for slot {}",
                        missing_slot
                    ));

                    network::send_message_forget(&self.proposers, &retry);
                }
                false => {
                    logger::log_debug(&format!(
                        "[Learner Agent] Skipping retry for slot {}",
                        missing_slot
                    ));
                }
            }
        }
    }

    fn handle_message(&self, message: NetworkMessage) -> NetworkMessage {
        match message.payload {
            MessagePayload::Learn(payload) => {
                logger::log_debug(&format!(
                    "Handling LEARN: slot={}, value={:?}",
                    payload.slot, payload.value
                ));
                if !self.config.acceptors_send_learns {
                    let learn_result = self.learner.learn(payload.slot, &payload.value);
                    match learn_result {
                        LearnResultTest::Execute(learns) => {
                            // TODO: Learns is a list of ready-to-execute-commands.
                            // This is where we would call the state machine

                            for learn_entry in learns {
                                let executed_value = learn_entry.value.clone();

                                // Execute command at "state machine"
                                let res = self.execute_command(executed_value.clone());

                                // quickfix let only one learner send to leader
                                // TODO: proposer should handle this but my brain is too small (wtf copilot)
                                // if self.node.node_id == 1 {
                                //     let network_message = NetworkMessage {
                                //         sender: self.node.clone(),
                                //         payload: MessagePayload::Ignore,
                                //     };
                                //     return network_message;
                                // }

                                let client_response = ClientResponse {
                                    client_id: executed_value.client_id.to_string(),
                                    client_seq: executed_value.client_seq,
                                    success: true,
                                    command_result: Some(res),
                                    slot: learn_entry.slot,
                                };

                                let network_message = NetworkMessage {
                                    sender: self.node.clone(),
                                    payload: MessagePayload::Executed(client_response),
                                };

                                network::send_message_forget(&self.proposers, &network_message);
                            }
                        }
                        LearnResultTest::Ignore => {
                            // Learend value that cannot be executed yet
                        }
                    }
                }
                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Learn(payload), // TODO: Use custom learn-ack type? Needed?
                }
            }
            other_message => {
                logger::log_debug(&format!(
                    "[Acceptor Agent] Received irrelevant message type: {:?}",
                    other_message
                ));
                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore,
                }
            }
        }
    }
}
