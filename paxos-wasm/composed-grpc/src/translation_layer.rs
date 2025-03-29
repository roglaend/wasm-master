use proto::paxos_proto;
use std::convert::TryFrom;

use crate::paxos_bindings::exports::paxos::default::paxos_coordinator;
use crate::paxos_bindings::paxos::default::network_types;
use crate::paxos_bindings::paxos::default::paxos_types;

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

// Conversion from internal network_types::NetworkMessage to proto::NetworkMessage.
impl From<network_types::NetworkMessage> for paxos_proto::NetworkMessage {
    fn from(wit_msg: network_types::NetworkMessage) -> Self {
        let kind = match wit_msg.kind {
            network_types::NetworkMessageKind::Prepare => {
                paxos_proto::network_message::NetworkMessageKind::Prepare as i32
            }
            network_types::NetworkMessageKind::Promise => {
                paxos_proto::network_message::NetworkMessageKind::Promise as i32
            }
            network_types::NetworkMessageKind::Accept => {
                paxos_proto::network_message::NetworkMessageKind::Accept as i32
            }
            network_types::NetworkMessageKind::Accepted => {
                paxos_proto::network_message::NetworkMessageKind::Accepted as i32
            }
            network_types::NetworkMessageKind::Learn => {
                paxos_proto::network_message::NetworkMessageKind::Learn as i32
            }
            network_types::NetworkMessageKind::Heartbeat => {
                paxos_proto::network_message::NetworkMessageKind::Heartbeat as i32
            }
            network_types::NetworkMessageKind::Ignore => {
                paxos_proto::network_message::NetworkMessageKind::Ignore as i32
            }
        };

        let payload = match wit_msg.payload {
            network_types::MessagePayload::Prepare(prep) => Some(
                paxos_proto::network_message::Payload::Prepare(paxos_proto::PreparePayload {
                    slot: prep.slot,
                    ballot: prep.ballot,
                }),
            ),
            network_types::MessagePayload::Promise(prom) => {
                let accepted = prom.accepted.into_iter().map(Into::into).collect();
                Some(paxos_proto::network_message::Payload::Promise(
                    paxos_proto::PromisePayload {
                        ballot: prom.ballot,
                        accepted,
                    },
                ))
            }
            network_types::MessagePayload::Accept(acc) => Some(
                paxos_proto::network_message::Payload::Accept(paxos_proto::AcceptPayload {
                    slot: acc.slot,
                    ballot: acc.ballot,
                    value: Some(acc.value.into()),
                }),
            ),
            network_types::MessagePayload::Accepted(accd) => Some(
                paxos_proto::network_message::Payload::Accepted(paxos_proto::AcceptedPayload {
                    slot: accd.slot,
                    ballot: accd.ballot,
                    success: accd.success,
                }),
            ),
            network_types::MessagePayload::Learn(learn) => Some(
                paxos_proto::network_message::Payload::Learn(paxos_proto::LearnPayload {
                    slot: learn.slot,
                    ballot: learn.ballot,
                    value: Some(learn.value.into()),
                }),
            ),
            network_types::MessagePayload::Heartbeat(hb) => Some(
                paxos_proto::network_message::Payload::Heartbeat(paxos_proto::HeartbeatPayload {
                    timestamp: hb.timestamp,
                }),
            ),
            network_types::MessagePayload::Empty => None,
        };

        let sender = Some(wit_msg.sender.into());
        paxos_proto::NetworkMessage {
            sender,
            kind,
            payload,
        }
    }
}

impl TryFrom<paxos_proto::NetworkMessage> for network_types::NetworkMessage {
    type Error = String;
    fn try_from(proto_msg: paxos_proto::NetworkMessage) -> Result<Self, Self::Error> {
        let kind = match proto_msg.kind {
            x if x == paxos_proto::network_message::NetworkMessageKind::Prepare as i32 => {
                network_types::NetworkMessageKind::Prepare
            }
            x if x == paxos_proto::network_message::NetworkMessageKind::Promise as i32 => {
                network_types::NetworkMessageKind::Promise
            }
            x if x == paxos_proto::network_message::NetworkMessageKind::Accept as i32 => {
                network_types::NetworkMessageKind::Accept
            }
            x if x == paxos_proto::network_message::NetworkMessageKind::Accepted as i32 => {
                network_types::NetworkMessageKind::Accepted
            }
            x if x == paxos_proto::network_message::NetworkMessageKind::Learn as i32 => {
                network_types::NetworkMessageKind::Learn
            }
            x if x == paxos_proto::network_message::NetworkMessageKind::Heartbeat as i32 => {
                network_types::NetworkMessageKind::Heartbeat
            }
            x if x == paxos_proto::network_message::NetworkMessageKind::Ignore as i32 => {
                network_types::NetworkMessageKind::Ignore
            }
            _ => return Err("Unknown network message kind".to_string()),
        };

        let proto_sender = proto_msg.sender.ok_or("Missing sender in proto message")?;
        let sender = paxos_types::Node::try_from(proto_sender)
            .map_err(|e| format!("Failed to convert Node: {}", e))?;

        let payload = match proto_msg.payload {
            Some(paxos_proto::network_message::Payload::Prepare(prep)) => {
                network_types::MessagePayload::Prepare(paxos_types::Prepare {
                    slot: prep.slot,
                    ballot: prep.ballot,
                })
            }
            Some(paxos_proto::network_message::Payload::Promise(prom)) => {
                let accepted = prom.accepted.into_iter().map(Into::into).collect();
                network_types::MessagePayload::Promise(paxos_types::Promise {
                    ballot: prom.ballot,
                    accepted,
                })
            }
            Some(paxos_proto::network_message::Payload::Accept(acc)) => {
                let value = acc
                    .value
                    .map(Into::into)
                    .unwrap_or_else(|| paxos_types::Value {
                        is_noop: true,
                        command: None,
                    });
                network_types::MessagePayload::Accept(paxos_types::Accept {
                    slot: acc.slot,
                    ballot: acc.ballot,
                    value,
                })
            }
            Some(paxos_proto::network_message::Payload::Accepted(accd)) => {
                network_types::MessagePayload::Accepted(paxos_types::Accepted {
                    slot: accd.slot,
                    ballot: accd.ballot,
                    success: accd.success,
                })
            }
            Some(paxos_proto::network_message::Payload::Learn(learn)) => {
                let value = learn
                    .value
                    .map(Into::into)
                    .unwrap_or_else(|| paxos_types::Value {
                        is_noop: true,
                        command: None,
                    });
                network_types::MessagePayload::Learn(paxos_types::Learn {
                    slot: learn.slot,
                    ballot: learn.ballot,
                    value,
                })
            }
            Some(paxos_proto::network_message::Payload::Heartbeat(hb)) => {
                network_types::MessagePayload::Heartbeat(network_types::Heartbeat {
                    node: sender.clone(),
                    timestamp: hb.timestamp,
                })
            }
            Some(paxos_proto::network_message::Payload::Ignore(_)) | None => {
                network_types::MessagePayload::Empty
            }
        };

        Ok(network_types::NetworkMessage {
            sender,
            kind,
            payload,
        })
    }
}

// Conversion from proto::NetworkResponse to internal network_types::NetworkResponse.
impl TryFrom<paxos_proto::NetworkResponse> for network_types::NetworkResponse {
    type Error = String;
    fn try_from(resp: paxos_proto::NetworkResponse) -> Result<Self, Self::Error> {
        let sender = paxos_types::Node::try_from(
            resp.sender
                .unwrap_or_else(|| panic!("Missing sender in proto response")),
        )
        .map_err(|e| format!("Failed to convert Node: {}", e))?;

        let kind = match resp.kind {
            x if x == paxos_proto::network_message::NetworkMessageKind::Prepare as i32 => {
                network_types::NetworkMessageKind::Prepare
            }
            x if x == paxos_proto::network_message::NetworkMessageKind::Promise as i32 => {
                network_types::NetworkMessageKind::Promise
            }
            x if x == paxos_proto::network_message::NetworkMessageKind::Accept as i32 => {
                network_types::NetworkMessageKind::Accept
            }
            x if x == paxos_proto::network_message::NetworkMessageKind::Accepted as i32 => {
                network_types::NetworkMessageKind::Accepted
            }
            x if x == paxos_proto::network_message::NetworkMessageKind::Learn as i32 => {
                network_types::NetworkMessageKind::Learn
            }
            x if x == paxos_proto::network_message::NetworkMessageKind::Heartbeat as i32 => {
                network_types::NetworkMessageKind::Heartbeat
            }
            x if x == paxos_proto::network_message::NetworkMessageKind::Ignore as i32 => {
                network_types::NetworkMessageKind::Ignore
            }
            _ => return Err("Unknown network message kind in response".to_string()),
        };

        let payload = match resp.payload {
            Some(paxos_proto::network_response::Payload::Prepare(prep)) => {
                network_types::MessagePayload::Prepare(paxos_types::Prepare {
                    slot: prep.slot,
                    ballot: prep.ballot,
                })
            }
            Some(paxos_proto::network_response::Payload::Promise(prom)) => {
                let accepted = prom.accepted.into_iter().map(Into::into).collect();
                network_types::MessagePayload::Promise(paxos_types::Promise {
                    ballot: prom.ballot,
                    accepted,
                })
            }
            Some(paxos_proto::network_response::Payload::Accept(acc)) => {
                network_types::MessagePayload::Accept(paxos_types::Accept {
                    slot: acc.slot,
                    ballot: acc.ballot,
                    value: acc.value.clone().map(Into::into).unwrap_or_else(|| {
                        paxos_types::Value {
                            is_noop: true,
                            command: None,
                        }
                    }),
                })
            }
            Some(paxos_proto::network_response::Payload::Accepted(accd)) => {
                network_types::MessagePayload::Accepted(paxos_types::Accepted {
                    slot: accd.slot,
                    ballot: accd.ballot,
                    success: accd.success,
                })
            }
            Some(paxos_proto::network_response::Payload::Learn(learn)) => {
                network_types::MessagePayload::Learn(paxos_types::Learn {
                    slot: learn.slot,
                    ballot: learn.ballot,
                    value: learn.value.clone().map(Into::into).unwrap_or_else(|| {
                        paxos_types::Value {
                            is_noop: true,
                            command: None,
                        }
                    }),
                })
            }
            Some(paxos_proto::network_response::Payload::Heartbeat(hb)) => {
                network_types::MessagePayload::Heartbeat(network_types::Heartbeat {
                    node: sender.clone(),
                    timestamp: hb.timestamp,
                })
            }
            Some(paxos_proto::network_response::Payload::Ignore(_)) | None => {
                network_types::MessagePayload::Empty
            }
        };

        let status = match resp.status {
            x if x == paxos_proto::network_response::StatusKind::Success as i32 => {
                network_types::StatusKind::Success
            }
            x if x == paxos_proto::network_response::StatusKind::Failure as i32 => {
                network_types::StatusKind::Failure
            }
            x if x == paxos_proto::network_response::StatusKind::Ignored as i32 => {
                network_types::StatusKind::Ignored
            }
            _ => network_types::StatusKind::Failure,
        };

        Ok(network_types::NetworkResponse {
            sender,
            kind,
            payload,
            status,
        })
    }
}

/// Conversion from internal network_types::NetworkResponse to proto::NetworkResponse.
impl From<network_types::NetworkResponse> for paxos_proto::NetworkResponse {
    fn from(internal: network_types::NetworkResponse) -> Self {
        let kind = match internal.kind {
            network_types::NetworkMessageKind::Prepare => {
                paxos_proto::network_message::NetworkMessageKind::Prepare as i32
            }
            network_types::NetworkMessageKind::Promise => {
                paxos_proto::network_message::NetworkMessageKind::Promise as i32
            }
            network_types::NetworkMessageKind::Accept => {
                paxos_proto::network_message::NetworkMessageKind::Accept as i32
            }
            network_types::NetworkMessageKind::Accepted => {
                paxos_proto::network_message::NetworkMessageKind::Accepted as i32
            }
            network_types::NetworkMessageKind::Learn => {
                paxos_proto::network_message::NetworkMessageKind::Learn as i32
            }
            network_types::NetworkMessageKind::Heartbeat => {
                paxos_proto::network_message::NetworkMessageKind::Heartbeat as i32
            }
            network_types::NetworkMessageKind::Ignore => {
                paxos_proto::network_message::NetworkMessageKind::Ignore as i32
            }
        };

        let payload = match internal.payload {
            network_types::MessagePayload::Prepare(prep) => Some(
                paxos_proto::network_response::Payload::Prepare(paxos_proto::PreparePayload {
                    slot: prep.slot,
                    ballot: prep.ballot,
                }),
            ),
            network_types::MessagePayload::Promise(prom) => {
                let accepted = prom.accepted.into_iter().map(Into::into).collect();
                Some(paxos_proto::network_response::Payload::Promise(
                    paxos_proto::PromisePayload {
                        ballot: prom.ballot,
                        accepted,
                    },
                ))
            }
            network_types::MessagePayload::Accept(acc) => Some(
                paxos_proto::network_response::Payload::Accept(paxos_proto::AcceptPayload {
                    slot: acc.slot,
                    ballot: acc.ballot,
                    value: Some(acc.value.into()),
                }),
            ),
            network_types::MessagePayload::Accepted(accd) => Some(
                paxos_proto::network_response::Payload::Accepted(paxos_proto::AcceptedPayload {
                    slot: accd.slot,
                    ballot: accd.ballot,
                    success: accd.success,
                }),
            ),
            network_types::MessagePayload::Learn(learn) => Some(
                paxos_proto::network_response::Payload::Learn(paxos_proto::LearnPayload {
                    slot: learn.slot,
                    ballot: learn.ballot,
                    value: Some(learn.value.into()),
                }),
            ),
            network_types::MessagePayload::Heartbeat(hb) => Some(
                paxos_proto::network_response::Payload::Heartbeat(paxos_proto::HeartbeatPayload {
                    timestamp: hb.timestamp,
                }),
            ),
            network_types::MessagePayload::Empty => None,
        };

        let sender = Some(internal.sender.into());
        let status = match internal.status {
            network_types::StatusKind::Success => {
                paxos_proto::network_response::StatusKind::Success as i32
            }
            network_types::StatusKind::Failure => {
                paxos_proto::network_response::StatusKind::Failure as i32
            }
            network_types::StatusKind::Ignored => {
                paxos_proto::network_response::StatusKind::Ignored as i32
            }
        };

        paxos_proto::NetworkResponse {
            sender,
            kind,
            payload,
            status,
        }
    }
}

/// Converts internal Paxos state (WIT type) to proto-defined PaxosState.
pub fn convert_internal_state_to_proto(
    internal_state: paxos_coordinator::PaxosState,
) -> paxos_proto::PaxosState {
    let learned = internal_state
        .learned
        .into_iter()
        .map(|entry| paxos_proto::LearnedEntry {
            slot: entry.slot,
            value: Some(entry.value.into()),
        })
        .collect();
    let kv_state = internal_state
        .kv_state
        .into_iter()
        .map(|pair| paxos_proto::KvPair {
            key: pair.key,
            value: Some(pair.value.into()),
        })
        .collect();
    paxos_proto::PaxosState { learned, kv_state }
}

/// Converts proto-defined PaxosState into internal Paxos state (WIT type).
pub fn _convert_proto_state_to_internal(
    proto_state: paxos_proto::PaxosState,
) -> paxos_coordinator::PaxosState {
    let learned = proto_state
        .learned
        .into_iter()
        .map(|entry| paxos_coordinator::LearnedEntry {
            slot: entry.slot,
            value: paxos_types::Value::try_from(entry.value.unwrap_or_default())
                .expect("Failed to convert Value for LearnedEntry"),
        })
        .collect();
    let kv_state = proto_state
        .kv_state
        .into_iter()
        .map(|pair| paxos_coordinator::KvPair {
            key: pair.key,
            value: paxos_types::Value::try_from(pair.value.unwrap_or_default())
                .expect("Failed to convert KvPair"),
        })
        .collect();
    paxos_coordinator::PaxosState { learned, kv_state }
}
