use std::sync::Arc;

pub mod bindings {
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
use bindings::paxos::default::learner_types::LearnResult;
use bindings::paxos::default::network_types::{Heartbeat, MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{
    Ballot, Learn, Node, PaxosRole, RunConfig, Slot, Value,
};
use bindings::paxos::default::{logger, network};

pub struct MyAcceptorAgent;

impl GuestAcceptorAgent for MyAcceptorAgent {
    type AcceptorAgentResource = MyAcceptorAgentResource;
}

pub struct MyAcceptorAgentResource {
    config: RunConfig,

    node: Node,
    learners: Vec<Node>,
    acceptor: Arc<AcceptorResource>,
}

impl MyAcceptorAgentResource {}

impl GuestAcceptorAgentResource for MyAcceptorAgentResource {
    fn new(node: Node, nodes: Vec<Node>, config: RunConfig) -> Self {
        let garbage_collection_window = Some(100);
        let acceptor = Arc::new(AcceptorResource::new(garbage_collection_window));

        // TODO: make this more future proof?
        let learners: Vec<_> = nodes
            .into_iter()
            .filter(|x| x.role == PaxosRole::Learner || x.role == PaxosRole::Coordinator)
            .collect();

        logger::log_info("[Acceptor Agent] Initialized core acceptor resource.");
        Self {
            config,
            node,
            learners,
            acceptor,
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
    fn commit_phase(&self, slot: Slot, value: Value) -> Option<LearnResult> {
        if self.config.acceptors_send_learns {
            logger::log_info(&format!(
                "[Acceptor Agent] Committing proposal: slot={}, value={:?}",
                slot, value
            ));
            let learn = Learn {
                slot,
                value: value.clone(),
            };

            // TODO: Copy pasted from paxos coordinator, not ready to be used, needs to be reconsidered.

            let learn_msg = NetworkMessage {
                sender: self.node.clone(),
                payload: MessagePayload::Learn(learn),
            };
            _ = network::send_message(&vec![], &learn_msg);
            Some(LearnResult {
                learned_value: value,
                quorum: self.learners.len() as u64, // TODO: This needed?
            })
        } else {
            logger::log_warn(
                "[Acceptor Agent] Attempted to broadcast learns, but ability not enabled.",
            );
            None
        }
    }

    fn handle_message(&self, message: NetworkMessage) -> NetworkMessage {
        logger::log_info(&format!(
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
                            network::send_message_forget(&vec![message.sender.clone()], &msg);
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
                    payload.slot, payload.ballot, payload.value
                ));
                let accepted_result =
                    self.process_accept(payload.slot, payload.ballot, payload.value);
                let response = match accepted_result {
                    AcceptedResult::Accepted(accepted) => {
                        let msg = NetworkMessage {
                            sender: self.node.clone(),
                            payload: MessagePayload::Accepted(accepted),
                        };

                        //* Fire-and-forget */
                        if self.config.is_event_driven {
                            network::send_message_forget(&vec![message.sender.clone()], &msg);
                        }
                        msg
                    }
                    AcceptedResult::Rejected(_ballot) => NetworkMessage {
                        sender: self.node.clone(),
                        payload: MessagePayload::Ignore, // TODO: Maybe give a better response?
                    },
                };
                response
            }
            MessagePayload::Heartbeat(payload) => {
                logger::log_info(&format!(
                    "[Acceptor Agent] Handling HEARTBEAT: sender: {:?}, timestamp={}",
                    payload.sender, payload.timestamp
                ));
                // Simply echo the heartbeat payload.
                let response_payload = Heartbeat {
                    sender: self.node.clone(),
                    // timestamp = ... // TODO: Have a consistent way to define these?
                    timestamp: payload.timestamp,
                };
                // TODO: Have a dedicated heartbeat ack payload type?
                let response = NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Heartbeat(response_payload),
                };
                //* Fire-and-forget */
                // if self.config.is_event_driven { // TODO: Needed?
                //     network::send_message_forget(&vec![message.sender.clone()], &response);
                // }
                response
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
}
