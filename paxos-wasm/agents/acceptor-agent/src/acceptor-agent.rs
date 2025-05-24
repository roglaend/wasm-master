use std::sync::Arc;

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "acceptor-agent-world",
        additional_derives: [PartialEq],
    });
}
bindings::export!(MyAcceptorAgent with_types_in bindings);

use bindings::exports::paxos::default::acceptor_agent::{
    Guest as GuestAcceptorAgent, GuestAcceptorAgentResource,
};
use bindings::paxos::default::acceptor::AcceptorResource;
use bindings::paxos::default::acceptor_types::{AcceptedResult, PromiseResult};
use bindings::paxos::default::network_types::{MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{
    Ballot, Learn, Node, PaxosRole, RunConfig, Slot, Value,
};
use bindings::paxos::default::{logger, network_client};

struct MyAcceptorAgent;

impl GuestAcceptorAgent for MyAcceptorAgent {
    type AcceptorAgentResource = MyAcceptorAgentResource;
}

struct MyAcceptorAgentResource {
    config: RunConfig,

    node: Node,
    learners: Vec<Node>,
    proposers: Vec<Node>,

    acceptor: Arc<AcceptorResource>,
    network_client: Arc<network_client::NetworkClientResource>,

    all_nodes: Vec<Node>,
}

impl MyAcceptorAgentResource {}

impl GuestAcceptorAgentResource for MyAcceptorAgentResource {
    fn new(node: Node, nodes: Vec<Node>, config: RunConfig) -> Self {
        let acceptor = Arc::new(AcceptorResource::new(&node.node_id.to_string(), config));

        let learners: Vec<_> = nodes
            .iter()
            .filter(|x| matches!(x.role, PaxosRole::Learner | PaxosRole::Coordinator))
            .cloned()
            .collect();

        let proposers: Vec<_> = nodes
            .clone()
            .into_iter()
            .filter(|x| x.role == PaxosRole::Proposer || x.role == PaxosRole::Coordinator)
            .collect();
        let network_client = Arc::new(network_client::NetworkClientResource::new());

        match acceptor.load_state() {
            Ok(_) => logger::log_info("[Acceptor Agent] Loaded state successfully."),
            Err(e) => logger::log_error(&format!(
                "[Acceptor Agent] Failed to load state. Ignore if first startup: {}",
                e
            )),
        }

        logger::log_info("[Acceptor Agent] Initialized core acceptor resource.");
        Self {
            config,
            node,
            learners,
            proposers,
            acceptor,
            network_client,
            all_nodes: nodes,
        }
    }

    // Processes a prepare request by directly delegating to the core acceptor.
    fn process_prepare(&self, slot: Slot, ballot: Ballot) -> PromiseResult {
        logger::log_info(&format!(
            "[Acceptor Agent] Processing PREPARE: slot={}, ballot={}",
            slot, ballot
        ));
        self.acceptor.prepare(slot, ballot)
    }

    // Processes an accept request by directly delegating to the core acceptor.
    fn process_accept(&self, slot: Slot, ballot: Ballot, value: Value) -> AcceptedResult {
        logger::log_info(&format!(
            "[Acceptor Agent] Processing ACCEPT: slot={}, ballot={}, value={:?}",
            slot, ballot, value
        ));
        self.acceptor.accept(slot, ballot, &value)
    }

    // Commit phase: broadcasts a learn message if configured to do so.
    fn commit_phase(&self, slot: Slot, value: Value) -> Option<Learn> {
        if self.config.acceptors_send_learns {
            logger::log_info(&format!(
                "[Acceptor Agent] Committing proposal: slot={}, value={:?}",
                slot, value
            ));
            let learn = Learn {
                slot,
                value: value.clone(),
            };

            let learn_msg = NetworkMessage {
                sender: self.node.clone(),
                payload: MessagePayload::Learn(learn.clone()),
            };
            self.network_client
                .send_message_forget(&self.learners, &learn_msg);
            Some(learn)
        } else {
            logger::log_warn(
                "[Acceptor Agent] Attempted to broadcast learns, but ability not enabled.",
            );
            panic!("Lol")
        }
    }

    fn retry_learn(&self, slot: Slot) {
        logger::log_info(&format!(
            "[Acceptor Agent] Retrying learn for slot {}",
            slot
        ));

        // Have accepted value, which didn't reach learner.
        // Or, no accepted value for this slot, missing from proposer.
        // Either get accepted value or noop.
        let value = self
            .acceptor
            .get_accepted(slot)
            .and_then(|p| p.value)
            .unwrap_or_else(|| Value {
                command: None,
                client_id: "".to_string(),
                client_seq: 0,
            });

        let learn = Learn { slot, value };

        let learn_msg = NetworkMessage {
            sender: self.node.clone(),
            payload: MessagePayload::Learn(learn.clone()),
        };

        // Broadcasts to all learners, since if one learner need a noop, then they all should?
        self.network_client
            .send_message_forget(&self.learners, &learn_msg);
    }

    fn handle_message(&self, message: NetworkMessage) -> NetworkMessage {
        logger::log_debug(&format!(
            "[Acceptor Agent] Received network message: {:?}",
            message
        ));

        // TODO: Fully implement and make use of the handling of incoming Promise and Accepted, when we use fire and forget messaging

        match message.payload {
            MessagePayload::Prepare(payload) => {
                logger::log_info(&format!(
                    "[Acceptor Agent] Handling PREPARE: slot={}, ballot={}",
                    payload.slot, payload.ballot
                ));
                let promise_result = self.process_prepare(payload.slot, payload.ballot);
                let response = match promise_result {
                    PromiseResult::Promised(promise) => {
                        let msg = NetworkMessage {
                            sender: self.node.clone(),
                            payload: MessagePayload::Promise(promise),
                        };

                        //* Fire-and-forget */
                        if self.config.is_event_driven {
                            self.network_client
                                .send_message_forget(&vec![message.sender], &msg);
                        }
                        msg
                    }
                    PromiseResult::Rejected(_ballot) => NetworkMessage {
                        sender: self.node.clone(),
                        payload: MessagePayload::Ignore, // TODO: Maybe give a better response?
                    },
                };
                response
            }
            MessagePayload::Accept(payload) => {
                logger::log_info(&format!(
                    "[Acceptor Agent] Handling ACCEPT: slot={}, ballot={}, value={:?}",
                    payload.slot, payload.ballot, &payload.value
                ));
                let accepted_result =
                    self.process_accept(payload.slot, payload.ballot, payload.value.clone());
                let response = match accepted_result {
                    AcceptedResult::Accepted(accepted) => {
                        let accepted_msg = NetworkMessage {
                            sender: self.node.clone(),
                            payload: MessagePayload::Accepted(accepted),
                        };
                        if self.config.is_event_driven {
                            if self.config.acceptors_send_learns {
                                let learn = Learn {
                                    slot: accepted.slot,
                                    value: payload.value,
                                };

                                let learn_msg = NetworkMessage {
                                    sender: self.node.clone(),
                                    payload: MessagePayload::Learn(learn),
                                };

                                self.network_client
                                    .send_message_forget(&self.learners, &learn_msg);
                            } else {
                                self.network_client
                                    .send_message_forget(&vec![message.sender], &accepted_msg);
                            }
                        }
                        accepted_msg
                    }
                    AcceptedResult::Rejected(_ballot) => NetworkMessage {
                        sender: self.node.clone(),
                        payload: MessagePayload::Ignore, // TODO: Maybe give a better response?
                    },
                };
                response
            }
            MessagePayload::RetryLearn(slot) => {
                logger::log_warn(&format!(
                    "[Acceptor Agent] Handling LEARN RETRY: slots={:?}",
                    slot
                ));

                self.retry_learn(slot);

                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore,
                }
            }

            // TODO: React to learn-ack if the acceptor agent broadcast learns?
            other_message => {
                logger::log_warn(&format!(
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

    fn send_heartbeat(&self) {
        let heartbeat_msg = NetworkMessage {
            sender: self.node.clone(),
            payload: MessagePayload::Heartbeat,
        };

        logger::log_info(&format!(
            "[Acceptor Agent] Sending heartbeat to all nodes: {:?}",
            self.all_nodes
        ));
        self.network_client
            .send_message_forget(&self.all_nodes, &heartbeat_msg);
    }
}
