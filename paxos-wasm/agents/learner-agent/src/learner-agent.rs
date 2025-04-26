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
use bindings::paxos::default::learner_types::{LearnResult, LearnerState};
use bindings::paxos::default::network_types::{Heartbeat, MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{
    Executed, KvPair, Node, PaxosRole, RunConfig, Slot, Value,
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
    // acceptors: Vec<Node>,
    learner: Arc<LearnerResource>,
    kv_store: Arc<KvStoreResource>,

    retries: RefCell<HashMap<Slot, Instant>>,
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
        let learner = Arc::new(LearnerResource::new());
        let kv_store = Arc::new(KvStoreResource::new());

        let proposers: Vec<_> = nodes
            .clone()
            .into_iter()
            .filter(|x| x.role == PaxosRole::Proposer || x.role == PaxosRole::Coordinator)
            .collect();

        // let acceptors: Vec<_> = nodes
        //     .clone()
        //     .into_iter()
        //     .filter(|x| x.role == PaxosRole::Acceptor || x.role == PaxosRole::Coordinator)
        //     .collect();

        logger::log_info("[Learner Agent] Initialized core acceptor resource.");
        Self {
            config,
            node,
            proposers,
            // acceptors,
            learner,
            kv_store,
            retries: RefCell::new(HashMap::new()),
        }
    }

    // TODO : fix proto for this so that it can actually be used
    fn get_state(&self) -> (LearnerState, Vec<KvPair>) {
        let learner_state = self.learner.get_state();
        let kv_store_state = self.kv_store.get_state();

        (learner_state, kv_store_state)
    }

    fn learn_and_execute(&self, slot: Slot, value: Value) -> Vec<Executed> {
        match self.learner.learn(slot, &value) {
            LearnResult::Execute(learns) => learns
                .into_iter()
                .map(|le| {
                    let res = self.kv_store.apply(&le.value.command);
                    Executed {
                        value: le.value.clone(),
                        slot: le.slot,
                        success: true,
                        cmd_result: res,
                    }
                })
                .collect(),
            LearnResult::Ignore => Vec::new(),
        }
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
                    "[Learner Agent] Handling LEARN: slot={}, value={:?}",
                    payload.slot, payload.value
                ));

                let executed_entries = self.learn_and_execute(payload.slot, payload.value.clone());

                if !executed_entries.is_empty() {
                    let exec_msg = NetworkMessage {
                        sender: self.node.clone(),
                        payload: MessagePayload::Executed(executed_entries),
                    };

                    let broadcast_and_ignore =
                        self.config.is_event_driven || self.config.acceptors_send_learns;

                    if broadcast_and_ignore {
                        network::send_message_forget(&self.proposers, &exec_msg);
                        return NetworkMessage {
                            sender: self.node.clone(),
                            payload: MessagePayload::Ignore, // TODO: Use custom learn-ack type? Needed?
                        };
                    } else {
                        return exec_msg;
                    }
                }

                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore, // TODO: Use custom learn-ack type? Needed?
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
