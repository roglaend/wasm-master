use proto::paxos_proto;
use std::convert::TryFrom;

use crate::paxos_bindings::exports::paxos::default::paxos_coordinator;
use crate::paxos_bindings::paxos::default::network;

/// Conversion from the proto-generated NetworkMessage to the WIT NetworkMessage.
impl TryFrom<paxos_proto::NetworkMessage> for network::NetworkMessage {
    type Error = String;
    fn try_from(proto_msg: paxos_proto::NetworkMessage) -> Result<Self, Self::Error> {
        use paxos_proto::network_message::{NetworkMessageKind, Payload};
        // Map the proto enum to the WIT kind.
        let kind = match proto_msg.kind {
            x if x == NetworkMessageKind::Prepare as i32 => network::NetworkMessageKind::Prepare,
            x if x == NetworkMessageKind::Promise as i32 => network::NetworkMessageKind::Promise,
            x if x == NetworkMessageKind::Accept as i32 => network::NetworkMessageKind::Accept,
            x if x == NetworkMessageKind::Accepted as i32 => network::NetworkMessageKind::Accepted,
            x if x == NetworkMessageKind::Commit as i32 => network::NetworkMessageKind::Commit,
            x if x == NetworkMessageKind::Heartbeat as i32 => network::NetworkMessageKind::Heartbeat,
            // x if x == NetworkMessageKind::Election as i32 => network::NetworkMessageKind::Election,
            // x if x == NetworkMessageKind::Leader as i32 => network::NetworkMessageKind::Leader,
            _ => return Err("Unknown network message kind".to_string()),
        };

        // Convert the oneof payload based on the kind.
        let payload = match proto_msg.kind {
            x if x == NetworkMessageKind::Prepare as i32 => {
                if let Some(Payload::Prepare(prep)) = proto_msg.payload {
                    network::MessagePayload::Prepare(network::PreparePayload {
                        sender: prep.sender,
                        slot: prep.slot,
                        ballot: prep.ballot,
                    })
                } else {
                    return Err("Missing Prepare payload".to_string());
                }
            }
            x if x == NetworkMessageKind::Promise as i32 => {
                if let Some(Payload::Promise(prom)) = proto_msg.payload {
                    // let accepted_value = if prom.accepted_value.is_empty() {
                    //     None
                    // } else {
                    //     Some(prom.accepted_value)
                    // };
                    network::MessagePayload::Promise(network::PromisePayload {
                        // slot: prom.slot,
                        ballot: prom.ballot,
                        // accepted_ballot: prom.accepted_ballot,
                        // accepted_value,
                        accepted: prom.accepted,
                    })
                } else {
                    return Err("Missing Promise payload".to_string());
                }
            }
            x if x == NetworkMessageKind::Accept as i32 => {
                if let Some(Payload::Accept(acc)) = proto_msg.payload {
                    network::MessagePayload::Accept(network::AcceptPayload {
                        sender: acc.sender,
                        slot: acc.slot,
                        ballot: acc.ballot,
                        proposal: acc.proposal,
                    })
                } else {
                    return Err("Missing Accept payload".to_string());
                }
            }
            x if x == NetworkMessageKind::Accepted as i32 => {
                if let Some(Payload::Accepted(accd)) = proto_msg.payload {
                    network::MessagePayload::Accepted(network::AcceptedPayload {
                        slot: accd.slot,
                        ballot: accd.ballot,
                        value: accd.value,
                    })
                } else {
                    return Err("Missing Accepted payload".to_string());
                }
            }
            x if x == NetworkMessageKind::Commit as i32 => {
                if let Some(Payload::Commit(comm)) = proto_msg.payload {
                    network::MessagePayload::Commit(network::CommitPayload {
                        slot: comm.slot,
                        value: comm.value,
                    })
                } else {
                    return Err("Missing Commit payload".to_string());
                }
            }
            x if x == NetworkMessageKind::Heartbeat as i32 => {
                if let Some(Payload::Heartbeat(hb)) = proto_msg.payload {
                    network::MessagePayload::Heartbeat(network::HeartbeatPayload {
                        nodeid: hb.nodeid,
                    })
                } else {
                    return Err("Missing Heartbeat payload".to_string());
                }
            }
            // x if x == NetworkMessageKind::Election as i32 => {
            //     if let Some(Payload::Election(el )) = proto_msg.payload {
            //         network::MessagePayload::Election(network::ElectionPayload {
            //             candidate_id: el.candidate_id,
            //         })
            //     } else {
            //         return Err("Missing Election payload".to_string());
            //     }
            // }
            // x if x == NetworkMessageKind::Leader as i32 => {
            //     if let Some(Payload::Leader(l )) = proto_msg.payload {
            //         network::MessagePayload::Leader(network::LeaderPayload {
            //             leader_id: l.leader_id,
            //         })
            //     } else {
            //         return Err("Missing Election payload".to_string());
            //     }
            // }

            _ => return Err("Unknown network message kind".to_string()),

            
        };

        Ok(network::NetworkMessage { kind, payload })
    }
}

/// Conversion from the WIT NetworkMessage to the proto-generated NetworkMessage.
impl From<network::NetworkMessage> for paxos_proto::NetworkMessage {
    fn from(wit_msg: network::NetworkMessage) -> Self {
        use paxos_proto::network_message::{NetworkMessageKind, Payload};
        let kind = match wit_msg.kind {
            network::NetworkMessageKind::Prepare => NetworkMessageKind::Prepare as i32,
            network::NetworkMessageKind::Promise => NetworkMessageKind::Promise as i32,
            network::NetworkMessageKind::Accept => NetworkMessageKind::Accept as i32,
            network::NetworkMessageKind::Accepted => NetworkMessageKind::Accepted as i32,
            network::NetworkMessageKind::Commit => NetworkMessageKind::Commit as i32,
            network::NetworkMessageKind::Heartbeat => NetworkMessageKind::Heartbeat as i32,
        //     network::NetworkMessageKind::Election => NetworkMessageKind::Election as i32,
        //     network::NetworkMessageKind::Leader => NetworkMessageKind::Leader as i32,
        };

        let payload = match wit_msg.payload {
            network::MessagePayload::Prepare(prep) => {
                Some(Payload::Prepare(paxos_proto::PreparePayload {
                    sender: prep.sender,
                    slot: prep.slot,
                    ballot: prep.ballot,
                }))
            }
            network::MessagePayload::Promise(prom) => {
                Some(Payload::Promise(paxos_proto::PromisePayload {
                    // slot: prom.slot,
                    ballot: prom.ballot,
                    accepted: prom.accepted,
                    // accepted_ballot: prom.accepted_ballot,
                    // accepted_value: prom.accepted_value.unwrap_or_default(),
                }))
            }
            network::MessagePayload::Accept(acc) => {
                Some(Payload::Accept(paxos_proto::AcceptPayload {
                    sender: acc.sender,
                    slot: acc.slot,
                    ballot: acc.ballot,
                    proposal: acc.proposal,
                }))
            }
            network::MessagePayload::Accepted(accd) => {
                Some(Payload::Accepted(paxos_proto::AcceptedPayload {
                    slot: accd.slot,
                    ballot: accd.ballot,
                    value: accd.value,
                }))
            }
            network::MessagePayload::Commit(comm) => {
                Some(Payload::Commit(paxos_proto::CommitPayload {
                    slot: comm.slot,
                    value: comm.value,
                }))
            }
            network::MessagePayload::Heartbeat(hb) => {
                Some(Payload::Heartbeat(paxos_proto::HeartbeatPayload {
                    nodeid: hb.nodeid,
                }))
            }
            // network::MessagePayload::Election(el) => {
            //     Some(Payload::Election(paxos_proto::ElectionPayload {
            //         candidate_id: el.candidate_id,
            //     }))
            // }
            // network::MessagePayload::Leader(l) => {
            //     Some(Payload::Leader(paxos_proto::LeaderPayload {
            //         leader_id: l.leader_id,
            //     }))
            // }
        };

        paxos_proto::NetworkMessage { kind, payload }
    }
}

/// Converts an internal Paxos state (WIT type) into the proto-defined PaxosState.
pub fn convert_internal_state_to_proto(
    internal_state: paxos_coordinator::PaxosState,
) -> paxos_proto::PaxosState {
    let learned = internal_state
        .learned
        .into_iter()
        .map(|entry| paxos_proto::LearnedEntry {
            slot: entry.slot,
            value: entry.value,
        })
        .collect();
    let kv_state = internal_state
        .kv_state
        .into_iter()
        .map(|pair| paxos_proto::KvPair {
            key: pair.key,
            value: pair.value,
        })
        .collect();
    paxos_proto::PaxosState { learned, kv_state }
}

/// Converts a proto-defined PaxosState into the internal Paxos state (WIT type).
pub fn _convert_proto_state_to_internal(
    proto_state: paxos_proto::PaxosState,
) -> paxos_coordinator::PaxosState {
    let learned = proto_state
        .learned
        .into_iter()
        .map(|entry| paxos_coordinator::LearnedEntry {
            slot: entry.slot,
            value: entry.value,
        })
        .collect();
    let kv_state = proto_state
        .kv_state
        .into_iter()
        .map(|pair| paxos_coordinator::KvPair {
            key: pair.key,
            value: pair.value,
        })
        .collect();
    paxos_coordinator::PaxosState { learned, kv_state }
}
