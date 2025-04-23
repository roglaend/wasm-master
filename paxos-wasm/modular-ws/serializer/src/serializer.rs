pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "serializer-world",
        additional_derives: [PartialEq, Clone],
    });
}

bindings::export!(MySerializer with_types_in bindings);

use bindings::exports::paxos::default::serializer::Guest;
// use bindings::paxos::default::logger;
use bindings::paxos::default::network_types::{Heartbeat, MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{
    Accept, Accepted, ClientResponse, Learn, Node, PaxosRole, Prepare, Promise, Value,
};

// The default node to use when deserialization fails.
// Adjust these fields as appropriate.
fn default_node() -> Node {
    Node {
        node_id: 0,
        address: String::new(),
        role: PaxosRole::Learner,
    }
}

/// Helper function that attempts to deserialize and returns a Result.
/// If any step fails, an error string is returned.
fn try_deserialize(serialized: Vec<u8>) -> Result<NetworkMessage, String> {
    if serialized.len() < 4 {
        return Err("Serialized message too short to contain length prefix".to_string());
    }

    // Extract the first 4 bytes and convert them to u32.
    let len_bytes: [u8; 4] = serialized[0..4]
        .try_into()
        .map_err(|_| "Failed to extract length prefix".to_string())?;
    let expected_len = u32::from_be_bytes(len_bytes) as usize;

    // Check that the remaining bytes match the expected length.
    let payload_bytes = &serialized[4..];
    if payload_bytes.len() != expected_len {
        return Err(format!(
            "Expected message length {}, but got {}",
            expected_len,
            payload_bytes.len()
        ));
    }

    // Convert the payload bytes into a String.
    let serialized_string = String::from_utf8(payload_bytes.to_vec())
        .map_err(|_| "Invalid UTF-8 sequence in payload".to_string())?;

    // Split the payload string on our chosen delimiter.
    let parts: Vec<&str> = serialized_string.split("||").collect();
    if parts.len() != 2 {
        return Err("Invalid serialized message format".to_string());
    }
    let sender_str = parts[0]
        .strip_prefix("sender=")
        .ok_or("Missing sender prefix".to_string())?;
    let payload_str = parts[1]
        .strip_prefix("payload=")
        .ok_or("Missing payload prefix".to_string())?;

    // Deserialize the individual components.
    let sender =
        deserialize_node(sender_str).map_err(|_| "Failed to deserialize node".to_string())?;
    let payload = deserialize_message_payload(payload_str)
        .map_err(|_| "Failed to deserialize payload".to_string())?;

    Ok(NetworkMessage { sender, payload })
}

pub struct MySerializer;

impl Guest for MySerializer {
    fn serialize(message: NetworkMessage) -> Vec<u8> {
        let sender_str = serialize_node(&message.sender);
        let payload_str = serialize_message_payload(&message.payload);
        let formatted = format!("sender={}||payload={}", sender_str, payload_str);
        let payload_bytes = formatted.into_bytes();

        // Compute length as a u32.
        let len: u32 = payload_bytes.len().try_into().expect("Message too long");

        // Allocate space: 4 bytes for the length prefix + the actual payload.
        let mut out = Vec::with_capacity(4 + payload_bytes.len());
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&payload_bytes);
        out
    }

    fn deserialize(serialized: Vec<u8>) -> NetworkMessage {
        match try_deserialize(serialized) {
            Ok(msg) => msg,
            Err(e) => {
                // logger::log_error(&format!("[TCP Serializer] Deserialization error: {}", e));
                // Return a dummy message that the server can recognize and ignore.
                panic!("[TCP Serializer] Deserialization error: {}", e);
                // NetworkMessage {
                //     sender: default_node(),
                //     payload: MessagePayload::Ignore,
                // }
            }
        }
    }
}

/// Serialize a Node.
fn serialize_node(n: &Node) -> String {
    let role_str = match n.role {
        PaxosRole::Coordinator => "coordinator",
        PaxosRole::Proposer => "proposer",
        PaxosRole::Acceptor => "acceptor",
        PaxosRole::Learner => "learner",
        PaxosRole::Client => "client",
    };
    format!(
        "node_id:{},address:{},role:{}",
        n.node_id, n.address, role_str
    )
}

/// Deserialize a Node.
fn deserialize_node(s: &str) -> Result<Node, &'static str> {
    let mut node_id: Option<u64> = None;
    let mut address: Option<String> = None;
    let mut role: Option<PaxosRole> = None;
    for part in s.split(',') {
        let mut kv = part.splitn(2, ':');
        let key = kv.next().unwrap_or("");
        let value = kv.next().unwrap_or("");
        match key {
            "node_id" => node_id = value.parse().ok(),
            "address" => address = Some(value.to_string()),
            "role" => {
                role = match value {
                    "coordinator" => Some(PaxosRole::Coordinator),
                    "proposer" => Some(PaxosRole::Proposer),
                    "acceptor" => Some(PaxosRole::Acceptor),
                    "learner" => Some(PaxosRole::Learner),
                    "client" => Some(PaxosRole::Client),
                    _ => None,
                }
            }
            _ => {}
        }
    }
    if let (Some(id), Some(addr), Some(r)) = (node_id, address, role) {
        Ok(Node {
            node_id: id,
            address: addr,
            role: r,
        })
    } else {
        
        Err("Failed to deserialize Node")
    }
}

/// Serialize MessagePayload.
fn serialize_message_payload(mp: &MessagePayload) -> String {
    match mp {
        MessagePayload::Ignore => "ignore".into(),
        MessagePayload::Prepare(p) => format!("prepare,slot:{},ballot:{}", p.slot, p.ballot),
        MessagePayload::Promise(pr) => format!("promise,slot:{},ballot:{}", pr.slot, pr.ballot),
        MessagePayload::Accept(a) => {
            let v = &a.value;
            let cmd = v.command.clone().unwrap_or_else(|| "none".into());
            format!(
                "accept,slot:{},ballot:{},is_noop:{},command:{},client_id:{},client_seq:{}",
                a.slot, a.ballot, v.is_noop, cmd, v.client_id, v.client_seq
            )
        }
        MessagePayload::Accepted(a) => {
            format!(
                "accepted,slot:{},ballot:{},success:{}",
                a.slot, a.ballot, a.success
            )
        }
        MessagePayload::Learn(l) => {
            let v = &l.value;
            let cmd = v.command.clone().unwrap_or_else(|| "none".into());
            format!(
                "learn,slot:{},is_noop:{},command:{},client_id:{},client_seq:{}",
                l.slot, v.is_noop, cmd, v.client_id, v.client_seq
            )
        }
        MessagePayload::Heartbeat(h) => {
            let sender_str = serialize_node(&h.sender);
            format!("heartbeat,sender:{},timestamp:{}", sender_str, h.timestamp)
        }
        MessagePayload::RetryLearn(id) => format!("retry-learn,{}", id),
        MessagePayload::Executed(cr) => {
            let cmd_res = cr.command_result.clone().unwrap_or_else(|| "none".into());
            format!(
                "executed,client_id:{},client_seq:{},success:{},command_result:{},slot:{}",
                cr.client_id, cr.client_seq, cr.success, cmd_res, cr.slot
            )
        }
        MessagePayload::ClientRequest(value) => {
            let cmd = value.command.clone().unwrap_or_else(|| "none".into());
            format!(
                "client-request,client_id:{},client_seq:{},command:{}",
                value.client_id, value.client_seq, cmd
            )
        }
        _ => "unknown".to_string(),
        // TODO: Add client request
    }
}

/// Deserialize MessagePayload.
fn deserialize_message_payload(s: &str) -> Result<MessagePayload, &'static str> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.is_empty() {
        return Err("Empty payload string");
    }
    match parts[0] {
        "ignore" => Ok(MessagePayload::Ignore),
        "prepare" => {
            let mut slot: Option<u64> = None;
            let mut ballot: Option<u64> = None;
            for part in &parts[1..] {
                let mut kv = part.splitn(2, ':');
                let key = kv.next().unwrap_or("");
                let value = kv.next().unwrap_or("");
                match key {
                    "slot" => slot = value.parse().ok(),
                    "ballot" => ballot = value.parse().ok(),
                    _ => {}
                }
            }
            if let (Some(s), Some(b)) = (slot, ballot) {
                Ok(MessagePayload::Prepare(Prepare { slot: s, ballot: b }))
            } else {
                Err("Failed to parse prepare payload")
            }
        }
        "promise" => {
            let mut slot: Option<u64> = None;
            let mut ballot: Option<u64> = None;
            for part in &parts[1..] {
                let mut kv = part.splitn(2, ':');
                let key = kv.next().unwrap_or("");
                let value = kv.next().unwrap_or("");
                match key {
                    "slot" => slot = value.parse().ok(),
                    "ballot" => ballot = value.parse().ok(),
                    _ => {}
                }
            }
            if let (Some(s), Some(b)) = (slot, ballot) {
                Ok(MessagePayload::Promise(Promise {
                    slot: s,
                    ballot: b,
                    accepted: vec![],
                }))
            } else {
                Err("Failed to parse promise payload")
            }
        }
        "accept" => {
            let mut slot: Option<u64> = None;
            let mut ballot: Option<u64> = None;
            let mut is_noop: Option<bool> = None;
            let mut command: Option<String> = None;
            let mut client_id: Option<u64> = None;
            let mut client_seq: Option<u64> = None;
            for part in &parts[1..] {
                let mut kv = part.splitn(2, ':');
                let key = kv.next().unwrap_or("");
                let value = kv.next().unwrap_or("");
                match key {
                    "slot" => slot = value.parse().ok(),
                    "ballot" => ballot = value.parse().ok(),
                    "is_noop" => is_noop = value.parse().ok(),
                    "command" => command = Some(value.to_string()),
                    "client_id" => client_id = value.parse().ok(),
                    "client_seq" => client_seq = value.parse().ok(),
                    _ => {}
                }
            }
            if let (Some(s), Some(b), Some(noop), Some(cmd), Some(cid), Some(cseq)) =
                (slot, ballot, is_noop, command, client_id, client_seq)
            {
                let val = Value {
                    is_noop: noop,
                    command: if cmd == "none" { None } else { Some(cmd) },
                    client_id: cid,
                    client_seq: cseq,
                };
                let a = Accept {
                    slot: s,
                    ballot: b,
                    value: val,
                };
                Ok(MessagePayload::Accept(a))
            } else {
                Err("Failed to parse accept payload")
            }
        }
        "accepted" => {
            let mut slot: Option<u64> = None;
            let mut ballot: Option<u64> = None;
            let mut success: Option<bool> = None;
            for part in &parts[1..] {
                let mut kv = part.splitn(2, ':');
                let key = kv.next().unwrap_or("");
                let value = kv.next().unwrap_or("");
                match key {
                    "slot" => slot = value.parse().ok(),
                    "ballot" => ballot = value.parse().ok(),
                    "success" => success = value.parse().ok(),
                    _ => {}
                }
            }
            if let (Some(s), Some(b), Some(suc)) = (slot, ballot, success) {
                Ok(MessagePayload::Accepted(Accepted {
                    slot: s,
                    ballot: b,
                    success: suc,
                }))
            } else {
                Err("Failed to parse accepted payload")
            }
        }
        "learn" => {
            let mut slot: Option<u64> = None;
            let mut is_noop: Option<bool> = None;
            let mut command: Option<String> = None;
            let mut client_id: Option<u64> = None;
            let mut client_seq: Option<u64> = None;
            for part in &parts[1..] {
                let mut kv = part.splitn(2, ':');
                let key = kv.next().unwrap_or("");
                let value = kv.next().unwrap_or("");
                match key {
                    "slot" => slot = value.parse().ok(),
                    "is_noop" => is_noop = value.parse().ok(),
                    "command" => command = Some(value.to_string()),
                    "client_id" => client_id = value.parse().ok(),
                    "client_seq" => client_seq = value.parse().ok(),
                    _ => {}
                }
            }
            if let (Some(s), Some(noop), Some(cmd), Some(cid), Some(cseq)) =
                (slot, is_noop, command, client_id, client_seq)
            {
                let val = Value {
                    is_noop: noop,
                    command: if cmd == "none" { None } else { Some(cmd) },
                    client_id: cid,
                    client_seq: cseq,
                };
                let l = Learn {
                    slot: s,
                    value: val,
                };
                Ok(MessagePayload::Learn(l))
            } else {
                Err("Failed to parse learn payload")
            }
        }
        "heartbeat" => {
            let mut sender_str = "";
            let mut timestamp: Option<u64> = None;
            for part in &parts[1..] {
                let mut kv = part.splitn(2, ':');
                let key = kv.next().unwrap_or("");
                let value = kv.next().unwrap_or("");
                match key {
                    "sender" => sender_str = value,
                    "timestamp" => timestamp = value.parse().ok(),
                    _ => {}
                }
            }
            if let Some(ts) = timestamp {
                let sender = deserialize_node(sender_str)?;
                let hb = Heartbeat {
                    sender,
                    timestamp: ts,
                };
                Ok(MessagePayload::Heartbeat(hb))
            } else {
                Err("Failed to parse heartbeat payload")
            }
        }
        "retry-learn" => {
            if parts.len() == 2 {
                if let Ok(id) = parts[1].parse() {
                    Ok(MessagePayload::RetryLearn(id))
                } else {
                    Err("Failed to parse retry-learn id")
                }
            } else {
                Err("Invalid retry-learn format")
            }
        }
        "executed" => {
            let mut client_id: Option<u64> = None;
            let mut client_seq: Option<u64> = None;
            let mut success: Option<bool> = None;
            let mut command_result: Option<String> = None;
            let mut slot: Option<u64> = None;
            for part in &parts[1..] {
                let mut kv = part.splitn(2, ':');
                let key = kv.next().unwrap_or("");
                let value = kv.next().unwrap_or("");
                match key {
                    "client_id" => client_id = value.parse().ok(),
                    "client_seq" => client_seq = value.parse().ok(),
                    "success" => success = value.parse().ok(),
                    "command_result" => command_result = Some(value.to_string()),
                    "slot" => slot = value.parse().ok(),
                    _ => {}
                }
            }
            if let (Some(cid), Some(cseq), Some(suc), Some(cmd), Some(s)) =
                (client_id, client_seq, success, command_result, slot)
            {
                let cr = ClientResponse {
                    client_id: cid.to_string(),
                    client_seq: cseq,
                    success: suc,
                    command_result: if cmd == "none" { None } else { Some(cmd) },
                    slot: s,
                };
                Ok(MessagePayload::Executed(cr))
            } else {
                Err("Failed to parse executed payload")
            }
        }
        "client-request" => {
             let mut client_id: Option<u64> = None;
             let mut client_seq: Option<u64> = None;
             let mut command: Option<String> = None;
             for part in &parts[1..] {
                 let mut kv = part.splitn(2, ':');
                 let key = kv.next().unwrap_or("");
                 let value = kv.next().unwrap_or("");
                 match key {
                     "client_id" => client_id = value.parse().ok(),
                     "client_seq" => client_seq = value.parse().ok(),
                     "command" => command = Some(value.to_string()),
                     _ => {}
                 }
             }
             if let (Some(cid), Some(cseq), Some(cmd)) =
                 (client_id, client_seq, command)
             {
                 Ok(MessagePayload::ClientRequest(Value {
                     is_noop: false,
                     client_id: cid,
                     client_seq: cseq,
                     command: if cmd == "none" { None } else { Some(cmd) },
                 }))
             } else {
                 Err("Failed to parse client-request payload")
             }
         }
        _ => Err("Unknown payload variant"),
    }
}
