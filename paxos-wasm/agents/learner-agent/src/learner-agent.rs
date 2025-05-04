use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "learner-agent-world",
        // generate_unused_types: true,
        additional_derives: [PartialEq, Clone],
    });
}

bindings::export!(MyLearnerAgent with_types_in bindings);

use bindings::exports::paxos::default::learner_agent::{
    Guest as GuestLearnerAgent, GuestLearnerAgentResource,
};

use bindings::paxos::default::kv_store::KvStoreResource;
use bindings::paxos::default::learner::LearnerResource;
use bindings::paxos::default::learner_types::{LearnResult, LearnerState, RetryLearnResult};
use bindings::paxos::default::network_types::{Heartbeat, MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{
    ExecuteResult, Executed, KvPair, Learn, Node, PaxosRole, RunConfig, Slot, Value,
};
use bindings::paxos::default::{logger, network};

pub struct MyLearnerAgent;

impl GuestLearnerAgent for MyLearnerAgent {
    type LearnerAgentResource = MyLearnerAgentResource;
}

pub struct MyLearnerAgentResource {
    config: RunConfig,

    node: Node,
    proposers: Vec<Node>,
    acceptors: Vec<Node>,
    learner: Arc<LearnerResource>,
    kv_store: Arc<KvStoreResource>,

    retries: RefCell<HashMap<Slot, Instant>>,
    retry_interval: Duration,
}

impl MyLearnerAgentResource {
    fn possible_retry(&self, slot: Slot, retry_interval: Duration) -> bool {
        match self.retries.borrow().get(&slot) {
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
        self.retries.borrow_mut().insert(slot, Instant::now());
        true
    }
}

impl GuestLearnerAgentResource for MyLearnerAgentResource {
    fn new(node: Node, nodes: Vec<Node>, config: RunConfig) -> Self {
        let learner = Arc::new(LearnerResource::new(&node.node_id.to_string()));
        let kv_store = Arc::new(KvStoreResource::new());

        match learner.load_state() {
            Ok(_) => logger::log_info("[Learner Agent] Loaded state successfully."),
            Err(e) => logger::log_error(&format!("[Learner Agent] Failed to load state: {}", e)),
        }

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

        let retry_interval = Duration::from_millis(10000); // TODO: Get from config

        logger::log_info("[Learner Agent] Initialized core acceptor resource.");
        Self {
            config,
            node,
            proposers,
            acceptors,
            learner,
            kv_store,
            retries: RefCell::new(HashMap::new()),
            retry_interval,
        }
    }

    // TODO : fix proto for this so that it can actually be used
    fn get_state(&self) -> (LearnerState, Vec<KvPair>) {
        let learner_state = self.learner.get_state();
        let kv_store_state = self.kv_store.get_state();

        (learner_state, kv_store_state)
    }

    fn get_next_to_execute(&self) -> Slot {
        self.learner.get_next_to_execute()
    }

    // This name is weird now - When acceptor send learns we do
    fn learn_and_execute(&self, slot: Slot, value: Value, from: Node) -> Executed {
        let mut execs: Vec<ExecuteResult> = Vec::new();

        if self.config.acceptors_send_learns {
            // Just check for ready to be executed slots
            let learn = Learn {
                slot,
                value: value.clone(),
            };

            self.learner.handle_learn(&learn, &from);

            if let LearnResult::Execute(learns) = self.learner.to_be_executed() {
                for le in learns {
                    let res = self.kv_store.apply(&le.value.command);
                    execs.push(ExecuteResult {
                        value: le.value.clone(),
                        slot: le.slot,
                        success: true,
                        cmd_result: res,
                    });
                }
            }
        } else {
            // Learn the slot first then check for to be executed slots
            self.learner.learn(slot, &value);
            if let LearnResult::Execute(learns) = self.learner.to_be_executed() {
                for le in learns {
                    let res = self.kv_store.apply(&le.value.command);
                    execs.push(ExecuteResult {
                        value: le.value.clone(),
                        slot: le.slot,
                        success: true,
                        cmd_result: res,
                    });
                }
            }
        }

        let adu = self.learner.get_next_to_execute() - 1;

        Executed {
            results: execs,
            adu,
        }
    }

    fn evaluate_retry(&self) -> RetryLearnResult {
        match self.learner.check_for_gap() {
            None => RetryLearnResult::NoGap,

            Some(slot) => {
                if self.possible_retry(slot, self.retry_interval) {
                    RetryLearnResult::Retry(slot)
                } else {
                    RetryLearnResult::Skip(slot)
                }
            }
        }
    }

    // Ticker called from host at a slower interval than proposer ticker
    fn run_paxos_loop(&self) {
        match self.evaluate_retry() {
            RetryLearnResult::NoGap => {
                // nothing to do
            }

            RetryLearnResult::Skip(slot) => {
                logger::log_debug(&format!("[Learner Agent] Skipping retry for slot {}", slot));
            }

            //* Assume event driven here */
            RetryLearnResult::Retry(slot) => {
                let retry_msg = NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::RetryLearn(slot),
                };
                logger::log_warn(&format!(
                    "[Learner Agent] Broadcasting RETRY LEARN for slot {}",
                    slot
                ));

                if self.config.acceptors_send_learns {
                    network::send_message_forget(&self.acceptors, &retry_msg);
                } else {
                    network::send_message_forget(&self.proposers, &retry_msg);
                }
            }
        }
        // let dummy_value = Value {
        //     command: None,
        //     client_id: "".to_string(),
        //     client_seq: 0,
        // };
        // let executed = self.learn_and_execute(0, dummy_value, self.node.clone());
        // if executed.results.is_empty() {
        //     return;
        // }
        // let exec_msg = NetworkMessage {
        //     sender: self.node.clone(),
        //     payload: MessagePayload::Executed(executed),
        // };

        // network::send_message_forget(&self.proposers, &exec_msg);
    }

    fn handle_message(&self, message: NetworkMessage) -> NetworkMessage {
        match message.payload {
            MessagePayload::Learn(payload) => {
                logger::log_debug(&format!(
                    "[Learner Agent] Handling LEARN: slot={}, value={:?}",
                    payload.slot, payload.value
                ));

                let learn = Learn {
                    slot: payload.slot,
                    value: payload.value.clone(),
                };

                self.learner.handle_learn(&learn, &message.sender);

                let executed =
                    self.learn_and_execute(payload.slot, payload.value.clone(), message.sender);

                if executed.results.is_empty() || !self.config.learners_send_executed {
                    return NetworkMessage {
                        sender: self.node.clone(),
                        payload: MessagePayload::Ignore,
                    }
                    .clone();
                }

                let exec_msg = NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Executed(executed),
                };

                network::send_message_forget(&self.proposers, &exec_msg);

                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore,
                }
            }

            other_message => {
                logger::log_debug(&format!(
                    "[Learner Agent] Received irrelevant message type: {:?}",
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
