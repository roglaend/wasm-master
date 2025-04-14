use proto::paxos_proto;
use std::convert::TryFrom;

// use crate::paxos_bindings::exports::paxos::default::paxos_coordinator;
use crate::learner::paxos_bindings::paxos::default::network_types;
use crate::learner::paxos_bindings::paxos::default::paxos_types;

// Conversion for Node.
impl From<paxos_types::Node> for paxos_proto::Node {
    fn from(node: paxos_types::Node) -> Self {
        paxos_proto::Node {
            node_id: node.node_id,
            address: node.address,
            role: match node.role {
                paxos_types::PaxosRole::Coordinator => paxos_proto::PaxosRole::Coordinator as i32,
                paxos_types::PaxosRole::Proposer => paxos_proto::PaxosRole::Proposer as i32,
                paxos_types::PaxosRole::Acceptor => paxos_proto::PaxosRole::Acceptor as i32,
                paxos_types::PaxosRole::Learner => paxos_proto::PaxosRole::Learner as i32,
            },
        }
    }
}

impl TryFrom<paxos_proto::Node> for paxos_types::Node {
    type Error = String;
    fn try_from(proto_node: paxos_proto::Node) -> Result<Self, Self::Error> {
        let role = match proto_node.role {
            x if x == paxos_proto::PaxosRole::Coordinator as i32 => {
                paxos_types::PaxosRole::Coordinator
            }
            x if x == paxos_proto::PaxosRole::Proposer as i32 => paxos_types::PaxosRole::Proposer,
            x if x == paxos_proto::PaxosRole::Acceptor as i32 => paxos_types::PaxosRole::Acceptor,
            x if x == paxos_proto::PaxosRole::Learner as i32 => paxos_types::PaxosRole::Learner,
            _ => return Err("Unknown PaxosRole".to_string()),
        };
        Ok(paxos_types::Node {
            node_id: proto_node.node_id,
            address: proto_node.address,
            role,
        })
    }
}

// Conversion for Value.
impl From<paxos_types::Value> for paxos_proto::Value {
    fn from(val: paxos_types::Value) -> Self {
        paxos_proto::Value {
            is_noop: val.is_noop,
            command: val.command.unwrap_or_default(),
            client_id: val.client_id,
            client_seq: val.client_seq,
        }
    }
}

impl From<paxos_proto::Value> for paxos_types::Value {
    fn from(value: paxos_proto::Value) -> Self {
        paxos_types::Value {
            is_noop: value.is_noop,
            command: if value.command.is_empty() {
                None
            } else {
                Some(value.command)
            },
            client_id: value.client_id,
            client_seq: value.client_seq,
        }
    }
}

// Conversions for PValue.
impl From<paxos_types::PValue> for paxos_proto::PValue {
    fn from(pv: paxos_types::PValue) -> Self {
        paxos_proto::PValue {
            slot: pv.slot,
            ballot: pv.ballot,
            value: pv.value.map(Into::into),
        }
    }
}

impl From<paxos_proto::PValue> for paxos_types::PValue {
    fn from(pv: paxos_proto::PValue) -> Self {
        paxos_types::PValue {
            slot: pv.slot,
            ballot: pv.ballot,
            value: pv.value.map(Into::into),
        }
    }
}
impl From<network_types::MessagePayload> for paxos_proto::network_message::Payload {
    fn from(payload: network_types::MessagePayload) -> Self {
        match payload {
            network_types::MessagePayload::Prepare(prep) => {
                paxos_proto::network_message::Payload::Prepare(paxos_proto::PreparePayload {
                    slot: prep.slot,
                    ballot: prep.ballot,
                })
            }
            network_types::MessagePayload::Promise(prom) => {
                let accepted = prom.accepted.into_iter().map(Into::into).collect();
                paxos_proto::network_message::Payload::Promise(paxos_proto::PromisePayload {
                    slot: prom.slot,
                    ballot: prom.ballot,
                    accepted,
                })
            }
            network_types::MessagePayload::Accept(acc) => {
                paxos_proto::network_message::Payload::Accept(paxos_proto::AcceptPayload {
                    slot: acc.slot,
                    ballot: acc.ballot,
                    value: Some(acc.value.into()),
                })
            }
            network_types::MessagePayload::Accepted(accd) => {
                paxos_proto::network_message::Payload::Accepted(paxos_proto::AcceptedPayload {
                    slot: accd.slot,
                    ballot: accd.ballot,
                    success: accd.success,
                })
            }
            network_types::MessagePayload::Learn(learn) => {
                paxos_proto::network_message::Payload::Learn(paxos_proto::LearnPayload {
                    slot: learn.slot,
                    // ballot: learn.ballot,
                    value: Some(learn.value.into()),
                })
            }
            network_types::MessagePayload::Heartbeat(hb) => {
                paxos_proto::network_message::Payload::Heartbeat(paxos_proto::HeartbeatPayload {
                    sender: Some(hb.sender.into()),
                    timestamp: hb.timestamp,
                })
            }
            network_types::MessagePayload::Ignore => {
                paxos_proto::network_message::Payload::Ignore(proto::paxos_proto::Empty {})
            }

            network_types::MessagePayload::RetryLearn(slot) => {
                paxos_proto::network_message::Payload::RetryLearn(paxos_proto::RetryLearnPayload {
                    slot,
                })
            }

            network_types::MessagePayload::Executed(val) => {
                paxos_proto::network_message::Payload::Executed(paxos_proto::ClientResponse {
                    client_id: val.client_id,
                    client_seq: val.client_seq, // TODO: Handle this properly
                    success: true,
                    command_result: val.command_result.unwrap_or_default(),
                    slot: val.slot,
                })
            }
            _ => {
                panic!("Unknown MessagePayload type");
            }
        }
    }
}

impl TryFrom<paxos_proto::network_message::Payload> for network_types::MessagePayload {
    type Error = String;
    fn try_from(proto_payload: paxos_proto::network_message::Payload) -> Result<Self, Self::Error> {
        match proto_payload {
            paxos_proto::network_message::Payload::Prepare(prep) => Ok(
                network_types::MessagePayload::Prepare(paxos_types::Prepare {
                    slot: prep.slot,
                    ballot: prep.ballot,
                }),
            ),
            paxos_proto::network_message::Payload::Promise(prom) => {
                let accepted = prom.accepted.into_iter().map(Into::into).collect();
                Ok(network_types::MessagePayload::Promise(
                    paxos_types::Promise {
                        slot: prom.slot,
                        ballot: prom.ballot,
                        accepted,
                    },
                ))
            }
            paxos_proto::network_message::Payload::Accept(acc) => {
                let value = acc
                    .value
                    .map(Into::into)
                    .unwrap_or_else(|| paxos_types::Value {
                        is_noop: true,
                        command: None,
                        client_id: 0,
                        client_seq: 0,
                    });
                Ok(network_types::MessagePayload::Accept(paxos_types::Accept {
                    slot: acc.slot,
                    ballot: acc.ballot,
                    value,
                }))
            }
            paxos_proto::network_message::Payload::Accepted(accd) => Ok(
                network_types::MessagePayload::Accepted(paxos_types::Accepted {
                    slot: accd.slot,
                    ballot: accd.ballot,
                    success: accd.success,
                }),
            ),
            paxos_proto::network_message::Payload::Learn(learn) => {
                let value = learn
                    .value
                    .map(Into::into)
                    .unwrap_or_else(|| paxos_types::Value {
                        is_noop: true,
                        command: None,
                        client_id: 0,
                        client_seq: 0,
                    });
                Ok(network_types::MessagePayload::Learn(paxos_types::Learn {
                    slot: learn.slot,
                    // ballot: learn.ballot,
                    value,
                }))
            }
            paxos_proto::network_message::Payload::Heartbeat(hb) => {
                let sender = hb
                    .sender
                    .ok_or("Missing sender in heartbeat payload")?
                    .try_into()?;
                Ok(network_types::MessagePayload::Heartbeat(
                    network_types::Heartbeat {
                        sender,
                        timestamp: hb.timestamp,
                    },
                ))
            }

            paxos_proto::network_message::Payload::Ignore(_) => {
                Ok(network_types::MessagePayload::Ignore)
            }

            paxos_proto::network_message::Payload::RetryLearn(retry) => {
                Ok(network_types::MessagePayload::RetryLearn(retry.slot))
            }

            paxos_proto::network_message::Payload::Executed(executed) => {
                Ok(network_types::MessagePayload::Executed(
                    paxos_types::ClientResponse {
                        client_id: executed.client_id,
                        client_seq: executed.client_seq, // TODO: Handle this properly
                        success: true,
                        command_result: Some(executed.command_result),
                        slot: executed.slot,
                    },
                ))
            }
        }
    }
}

// Conversion for NetworkMessage
impl From<network_types::NetworkMessage> for paxos_proto::NetworkMessage {
    fn from(wit_msg: network_types::NetworkMessage) -> Self {
        let sender = Some(wit_msg.sender.into());
        let payload = Some(wit_msg.payload.into());
        paxos_proto::NetworkMessage { sender, payload }
    }
}

impl TryFrom<paxos_proto::NetworkMessage> for network_types::NetworkMessage {
    type Error = String;
    fn try_from(proto_msg: paxos_proto::NetworkMessage) -> Result<Self, Self::Error> {
        let proto_sender = proto_msg.sender.ok_or("Missing sender in proto message")?;
        let sender = paxos_types::Node::try_from(proto_sender)
            .map_err(|e| format!("Failed to convert Node: {}", e))?;
        let payload = match proto_msg.payload {
            Some(pl) => network_types::MessagePayload::try_from(pl)?,
            None => network_types::MessagePayload::Ignore,
        };
        Ok(network_types::NetworkMessage { sender, payload })
    }
}

// pub fn convert_internal_state_to_proto(
//     internal_state: paxos::LearnerState,
// ) -> paxos_proto::PaxosState {
//     let learned = internal_state
//         .learned
//         .into_iter()
//         .map(|entry| paxos_proto::LearnedEntry {
//             slot: entry.slot,
//             value: Some(entry.value.into()),
//         })
//         .collect();
//     let kv_state = internal_state
//         .kv_store
//         .into_iter()
//         .map(|pair| paxos_proto::KvPair {
//             key: pair.key,
//             value: Some(pair.value.into()),
//         })
//         .collect();
//     paxos_proto::PaxosState { learned, kv_state }
// }

// Converts proto-defined PaxosState into internal Paxos state (WIT type).
// pub fn _convert_proto_state_to_internal(
//     proto_state: paxos_proto::PaxosState,
// ) -> paxos_coordinator::PaxosState {
//     let learned = proto_state
//         .learned
//         .into_iter()
//         .map(|entry| paxos_coordinator::LearnedEntry {
//             slot: entry.slot,
//             value: paxos_types::Value::try_from(entry.value.unwrap_or_default())
//                 .expect("Failed to convert Value for LearnedEntry"),
//         })
//         .collect();
//     let kv_state = proto_state
//         .kv_state
//         .into_iter()
//         .map(|pair| paxos_coordinator::KvPair {
//             key: pair.key,
//             value: paxos_types::Value::try_from(pair.value.unwrap_or_default())
//                 .expect("Failed to convert KvPair"),
//         })
//         .collect();
//     paxos_coordinator::PaxosState { learned, kv_store: kv_state }
// }
// // /// Converts internal Paxos state (WIT type) to proto-defined PaxosState.
// pub fn convert_internal_state_to_proto(
//     internal_state: paxos_coordinator::PaxosState,
// ) -> paxos_proto::PaxosState {
//     let learned = internal_state
//         .learned
//         .into_iter()
//         .map(|entry| paxos_proto::LearnedEntry {
//             slot: entry.slot,
//             value: Some(entry.value.into()),
//         })
//         .collect();
//     let kv_state = internal_state
//         .kv_state
//         .into_iter()
//         .map(|pair| paxos_proto::KvPair {
//             key: pair.key,
//             value: Some(pair.value.into()),
//         })
//         .collect();
//     paxos_proto::PaxosState { learned, kv_state }
// }

// /// Converts proto-defined PaxosState into internal Paxos state (WIT type).
// pub fn _convert_proto_state_to_internal(
//     proto_state: paxos_proto::PaxosState,
// ) -> paxos_coordinator::PaxosState {
//     let learned = proto_state
//         .learned
//         .into_iter()
//         .map(|entry| paxos_coordinator::LearnedEntry {
//             slot: entry.slot,
//             value: paxos_types::Value::try_from(entry.value.unwrap_or_default())
//                 .expect("Failed to convert Value for LearnedEntry"),
//         })
//         .collect();
//     let kv_state = proto_state
//         .kv_state
//         .into_iter()
//         .map(|pair| paxos_coordinator::KvPair {
//             key: pair.key,
//             value: paxos_types::Value::try_from(pair.value.unwrap_or_default())
//                 .expect("Failed to convert KvPair"),
//         })
//         .collect();
//     paxos_coordinator::PaxosState { learned, kv_state }
// }
