use crate::bindings::paxos::default::network_types::{MessagePayload, NetworkMessage};
use crate::bindings::paxos::default::paxos_types::{
    Accept, Accepted, ClientResponse, CmdResult, ExecuteResult, Executed, KvPair, Learn, Node,
    Operation, PValue, PaxosRole, Prepare, Promise, RetryLearns, Value,
};

pub trait HostSerializer {
    fn serialize(message: NetworkMessage) -> Vec<u8>;
    fn deserialize(bytes: Vec<u8>) -> Result<NetworkMessage, String>;
}

/// Native Rust implementation of the Serializer

#[derive(Clone)]
pub struct MyHostSerializer;

impl HostSerializer for MyHostSerializer {
    fn serialize(message: NetworkMessage) -> Vec<u8> {
        let sender_str = serialize_node(&message.sender);
        let payload_str = serialize_message_payload(&message.payload);
        let formatted = format!("sender={}||payload={}", sender_str, payload_str);
        let payload_bytes = formatted.into_bytes();

        let len: u32 = payload_bytes.len().try_into().expect("Message too long");
        let mut out = Vec::with_capacity(4 + payload_bytes.len());
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&payload_bytes);
        out
    }

    fn deserialize(serialized: Vec<u8>) -> Result<NetworkMessage, String> {
        try_deserialize(serialized)
    }
}

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
        .map_err(|e| format!("Failed to deserialize payload: {}", e))?;

    Ok(NetworkMessage { sender, payload })
}

fn serialize_node(n: &Node) -> String {
    let role_str = match n.role {
        PaxosRole::Proposer => "proposer",
        PaxosRole::Acceptor => "acceptor",
        PaxosRole::Learner => "learner",
        PaxosRole::Client => "client",
        PaxosRole::Coordinator => "coordinator",
    };
    format!(
        "node_id:{},address:{},role:{}",
        n.node_id, n.address, role_str
    )
}
/// Deserialize a Node
fn deserialize_node(s: &str) -> Result<Node, &'static str> {
    let mut node_id = None;
    let mut address = None;
    let mut role = None;

    for part in s.split(',') {
        let mut kv = part.splitn(2, ':');
        match (kv.next(), kv.next()) {
            (Some("node_id"), Some(v)) => node_id = v.parse().ok(),
            (Some("address"), Some(v)) => address = Some(v.to_string()),
            (Some("role"), Some(v)) => {
                role = Some(match v {
                    "proposer" => PaxosRole::Proposer,
                    "acceptor" => PaxosRole::Acceptor,
                    "learner" => PaxosRole::Learner,
                    "client" => PaxosRole::Client,
                    "coordinator" => PaxosRole::Coordinator,
                    _ => return Err("unknown role"),
                });
            }
            _ => {}
        }
    }

    match (node_id, address, role) {
        (Some(id), Some(addr), Some(r)) => Ok(Node {
            node_id: id,
            address: addr,
            role: r,
        }),
        _ => Err("missing node_id, address, or role"),
    }
}

fn serialize_cmd_result(cr: &CmdResult) -> String {
    match cr {
        CmdResult::NoOp => "no-op".into(),
        CmdResult::CmdValue(None) => "cmd-value:none".into(),
        CmdResult::CmdValue(Some(v)) => format!("cmd-value:{}", v),
    }
}

fn deserialize_cmd_result(s: &str) -> Result<CmdResult, &'static str> {
    if s == "no-op" {
        Ok(CmdResult::NoOp)
    } else if let Some(rest) = s.strip_prefix("cmd-value:") {
        // "cmd-value:none" or "cmd-value:<some>"
        if rest == "none" {
            Ok(CmdResult::CmdValue(None))
        } else {
            Ok(CmdResult::CmdValue(Some(rest.to_string())))
        }
    } else {
        Err("bad cmd-result")
    }
}

/// Serialize a MessagePayload to a simple comma-delimited string.
fn serialize_message_payload(mp: &MessagePayload) -> String {
    match mp {
        MessagePayload::Ignore => "ignore".into(),
        MessagePayload::ClientRequest(cr) => {
            let v = &cr; // TODO: Change back to ClientRequest if needed.
            let op = v
                .command
                .as_ref()
                .map_or("none".into(), |o| serialize_operation(o));
            format!(
                "client-request,client_id:{},client_seq:{},op:{}",
                v.client_id, v.client_seq, op
            )
        }
        MessagePayload::ClientResponse(cr) => {
            let result = serialize_cmd_result(&cr.command_result);
            format!(
                "client-response,client_id:{},client_seq:{},success:{},result:{}",
                cr.client_id, cr.client_seq, cr.success, result
            )
        }
        MessagePayload::Prepare(p) => {
            format!("prepare,slot:{},ballot:{}", p.slot, p.ballot)
        }
        MessagePayload::Promise(pr) => {
            // turn each PValue into a mini-record "slot:…,ballot:…,client_id:…,client_seq:…,op:…"
            let accepted_entries = pr
                .accepted
                .iter()
                .map(|pv| {
                    // unwrap the Option<Value> inside the p-value
                    let (cid, cseq, op) = if let Some(v) = &pv.value {
                        let op = v
                            .command
                            .as_ref()
                            .map_or("none".into(), |o| serialize_operation(o));

                        (v.client_id.clone(), v.client_seq, op)
                    } else {
                        // no prior accepted value
                        ("unknown".into(), 0, "none".into())
                    };
                    format!(
                        "slot:{},ballot:{},client_id:{},client_seq:{},op:{}",
                        pv.slot, pv.ballot, cid, cseq, op
                    )
                })
                .collect::<Vec<_>>()
                .join("|");

            if accepted_entries.is_empty() {
                // no prior accepts
                format!("promise,slot:{},ballot:{}", pr.slot, pr.ballot)
            } else {
                // embed as a bracketed list
                format!(
                    "promise,slot:{},ballot:{},accepted:[{}]",
                    pr.slot, pr.ballot, accepted_entries
                )
            }
        }
        MessagePayload::Accept(a) => {
            let v = &a.value;
            let op = v
                .command
                .as_ref()
                .map_or("none".into(), |o| serialize_operation(o));
            format!(
                "accept,slot:{},ballot:{},client_id:{},client_seq:{},op:{}",
                a.slot, a.ballot, v.client_id, v.client_seq, op
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
            let op = v
                .command
                .as_ref()
                .map_or("none".into(), |o| serialize_operation(o));
            format!(
                "learn,slot:{},client_id:{},client_seq:{},op:{}",
                l.slot, v.client_id, v.client_seq, op
            )
        }
        MessagePayload::Heartbeat => "heartbeat".into(),
        MessagePayload::RetryLearns(retries) => {
            let slots = retries
                .slots
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join("|");
            format!("retry-learns,slots:{}", slots)
        }
        MessagePayload::Executed(exec) => {
            let entries = exec
                .results
                .iter()
                .map(|r| {
                    let v = &r.value;
                    let op = v
                        .command
                        .as_ref()
                        .map_or("none".into(), |o| serialize_operation(o));
                    let result = serialize_cmd_result(&r.cmd_result);
                    format!(
                        "slot:{},client_id:{},client_seq:{},op:{},success:{},result:{}",
                        r.slot, v.client_id, v.client_seq, op, r.success, result
                    )
                })
                .collect::<Vec<_>>()
                .join("|");

            format!("executed,adu:{},results:[{}]", exec.adu, entries)
        }
        MessagePayload::Benchmark(b) => {
            format!(
                "benchmark,send_timestamp:{},payload:{}",
                b.send_timestamp, b.payload
            )
        }
        MessagePayload::Adu(adu) => {
            // Serialize ADU as a simple string
            format!("adu,{}", adu)
        }
    }
}

/// Deserialize MessagePayload.
fn deserialize_message_payload(s: &str) -> Result<MessagePayload, &'static str> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.is_empty() {
        return Err("empty payload");
    }
    // logger::log_debug(&format!(
    //     "[Serializer] Message payload to deserialize: {}",
    //     s
    // ));
    match parts[0] {
        "ignore" => Ok(MessagePayload::Ignore),
        "client-request" => {
            let mut client_id = None;
            let mut client_seq = None;
            let mut op = None;
            for part in &parts[1..] {
                let mut kv = part.splitn(2, ':');
                match (kv.next(), kv.next()) {
                    (Some("client_id"), Some(v)) => client_id = v.parse().ok(),
                    (Some("client_seq"), Some(v)) => client_seq = v.parse().ok(),
                    (Some("op"), Some(v)) => op = Some(v.to_string()),
                    _ => {}
                }
            }
            if let (Some(cid), Some(cseq), Some(op)) = (client_id, client_seq, op) {
                let cmd = if op == "none" {
                    None
                } else {
                    // parse Operation from its Debug string
                    Some(deserialize_operation(&op).map_err(|_| "bad op")?)
                };
                let v = Value {
                    command: cmd,
                    client_id: cid,
                    client_seq: cseq,
                };
                Ok(MessagePayload::ClientRequest(v)) // TODO: Change back to ClientRequest if needed
            } else {
                Err("bad client-request")
            }
        }
        "client-response" => {
            let mut client_id = None;
            let mut client_seq = None;
            let mut success = None;
            let mut result = None;
            for part in &parts[1..] {
                let mut kv = part.splitn(2, ':');
                match (kv.next(), kv.next()) {
                    (Some("client_id"), Some(v)) => client_id = Some(v.to_string()),
                    (Some("client_seq"), Some(v)) => client_seq = v.parse().ok(),
                    (Some("success"), Some(v)) => success = v.parse().ok(),
                    (Some("result"), Some(v)) => result = Some(v.to_string()),
                    _ => {}
                }
            }
            if let (Some(cid), Some(cseq), Some(suc), Some(res_s)) =
                (client_id, client_seq, success, result)
            {
                let cmd_result = deserialize_cmd_result(&res_s)?;
                Ok(MessagePayload::ClientResponse(ClientResponse {
                    client_id: cid,
                    client_seq: cseq,
                    success: suc,
                    command_result: cmd_result,
                }))
            } else {
                Err("bad client-response")
            }
        }
        "prepare" => {
            let mut slot = None;
            let mut ballot = None;
            for p in &parts[1..] {
                let mut kv = p.splitn(2, ':');
                match (kv.next(), kv.next()) {
                    (Some("slot"), Some(v)) => slot = v.parse().ok(),
                    (Some("ballot"), Some(v)) => ballot = v.parse().ok(),
                    _ => {}
                }
            }
            if let (Some(s), Some(b)) = (slot, ballot) {
                Ok(MessagePayload::Prepare(Prepare { slot: s, ballot: b }))
            } else {
                Err("bad prepare")
            }
        }
        "promise" => {
            // full raw string
            let raw = s;

            // first, pull out an accepted-list if present
            let mut accepted = Vec::new();
            let (hdr, list_str) = if let Some(start) = raw.find("accepted:[") {
                let hdr = &raw[..start]; // e.g. "promise,slot:5,ballot:10,"
                let inner_start = start + "accepted:[".len();
                let inner_end = raw[inner_start..]
                    .find(']')
                    .ok_or("malformed promise accepted list")?
                    + inner_start;
                let list = &raw[inner_start..inner_end]; // e.g. "slot:3,ballot:8,client_id:7,client_seq:42,op:get(\"foo\")|…"
                (hdr, Some(list))
            } else {
                (raw, None)
            };

            // parse slot & ballot from the header
            let mut slot = None;
            let mut ballot = None;
            for part in hdr.split(',').skip(1) {
                let mut kv = part.splitn(2, ':');
                match (kv.next(), kv.next()) {
                    (Some("slot"), Some(v)) => slot = v.parse().ok(),
                    (Some("ballot"), Some(v)) => ballot = v.parse().ok(),
                    _ => {}
                }
            }
            // if there is an accepted list, parse each `entry1|entry2|…`
            if let Some(list) = list_str {
                for entry in list.split('|') {
                    let mut es = None;
                    let mut eb = None;
                    let mut cid = None;
                    let mut cseq = None;
                    let mut op = None;

                    for field in entry.split(',') {
                        let mut kv = field.splitn(2, ':');
                        match (kv.next(), kv.next()) {
                            (Some("slot"), Some(v)) => es = v.parse().ok(),
                            (Some("ballot"), Some(v)) => eb = v.parse().ok(),
                            (Some("client_id"), Some(v)) => cid = v.parse().ok(),
                            (Some("client_seq"), Some(v)) => cseq = v.parse().ok(),
                            (Some("op"), Some(v)) => op = Some(v.to_string()),
                            _ => {}
                        }
                    }

                    if let (Some(es), Some(eb), Some(cid), Some(cseq), Some(opstr)) =
                        (es, eb, cid, cseq, op)
                    {
                        // parse the op (or treat `none` as no-value)
                        let val = if opstr == "none" {
                            None
                        } else {
                            let oper = deserialize_operation(&opstr)
                                .map_err(|_| "bad op in promise.accepted")?;
                            Some(Value {
                                command: Some(oper),
                                client_id: cid,
                                client_seq: cseq,
                            })
                        };

                        accepted.push(PValue {
                            slot: es,
                            ballot: eb,
                            value: val,
                        });
                    }
                }
            }
            // finally, construct the Promise
            if let (Some(s), Some(b)) = (slot, ballot) {
                Ok(MessagePayload::Promise(Promise {
                    slot: s,
                    ballot: b,
                    accepted, // possibly empty
                }))
            } else {
                Err("bad promise")
            }
        }
        "accept" => {
            let mut slot = None;
            let mut ballot = None;
            let mut client_id = None;
            let mut client_seq = None;
            let mut op = None;
            for p in &parts[1..] {
                let mut kv = p.splitn(2, ':');
                match (kv.next(), kv.next()) {
                    (Some("slot"), Some(v)) => slot = v.parse().ok(),
                    (Some("ballot"), Some(v)) => ballot = v.parse().ok(),
                    (Some("client_id"), Some(v)) => client_id = v.parse().ok(),
                    (Some("client_seq"), Some(v)) => client_seq = v.parse().ok(),
                    (Some("op"), Some(v)) => op = Some(v.to_string()),
                    _ => {}
                }
            }
            if let (Some(s), Some(b), Some(cid), Some(cseq), Some(op)) =
                (slot, ballot, client_id, client_seq, op)
            {
                let cmd = if op == "none" {
                    None
                } else {
                    Some(deserialize_operation(&op).map_err(|_| "bad op")?)
                };
                let v = Value {
                    command: cmd,
                    client_id: cid,
                    client_seq: cseq,
                };
                Ok(MessagePayload::Accept(Accept {
                    slot: s,
                    ballot: b,
                    value: v,
                }))
            } else {
                Err("bad accept")
            }
        }

        "accepted" => {
            let mut slot = None;
            let mut ballot = None;
            let mut success = None;
            for p in &parts[1..] {
                let mut kv = p.splitn(2, ':');
                match (kv.next(), kv.next()) {
                    (Some("slot"), Some(v)) => slot = v.parse().ok(),
                    (Some("ballot"), Some(v)) => ballot = v.parse().ok(),
                    (Some("success"), Some(v)) => success = v.parse().ok(),
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
                Err("bad accepted")
            }
        }

        "learn" => {
            let mut slot = None;
            let mut client_id = None;
            let mut client_seq = None;
            let mut op = None;
            for p in &parts[1..] {
                let mut kv = p.splitn(2, ':');
                match (kv.next(), kv.next()) {
                    (Some("slot"), Some(v)) => slot = v.parse().ok(),
                    (Some("client_id"), Some(v)) => client_id = v.parse().ok(),
                    (Some("client_seq"), Some(v)) => client_seq = v.parse().ok(),
                    (Some("op"), Some(v)) => op = Some(v.to_string()),
                    _ => {}
                }
            }
            if let (Some(s), Some(cid), Some(cseq), Some(op)) = (slot, client_id, client_seq, op) {
                let cmd = if op == "none" {
                    None
                } else {
                    Some(deserialize_operation(&op).map_err(|_| "bad op")?)
                };
                let v = Value {
                    command: cmd,
                    client_id: cid,
                    client_seq: cseq,
                };
                Ok(MessagePayload::Learn(Learn { slot: s, value: v }))
            } else {
                Err("bad learn")
            }
        }

        "heartbeat" => Ok(MessagePayload::Heartbeat),

        "retry-learns" => {
            // expect exactly one more part: "slots:3|4|5"
            if parts.len() == 2 && parts[1].starts_with("slots:") {
                let list = &parts[1]["slots:".len()..];
                let mut slots = Vec::new();
                for tok in list.split('|') {
                    let slot = tok.parse::<u64>().map_err(|_| "bad slot")?;
                    slots.push(slot);
                }
                Ok(MessagePayload::RetryLearns(RetryLearns { slots }))
            } else {
                Err("bad retry-learns")
            }
        }

        "executed" => {
            // full raw payload string
            let raw = s;

            // 1) Split off the results list if present
            let (hdr, list_str_opt) = if let Some(start) = raw.find("results:[") {
                let hdr = &raw[..start]; // e.g. "executed,adu:5,"
                let inner_start = start + "results:[".len();
                let inner_end = raw[inner_start..]
                    .find(']')
                    .ok_or("malformed executed results list")?
                    + inner_start;
                let list = &raw[inner_start..inner_end]; // e.g. "slot:3,client_id:7,...|slot:4,..."
                (hdr, Some(list))
            } else {
                (raw, None)
            };

            // 2) Parse the ADU from the header
            let mut adu: Option<u64> = None;
            for part in hdr.split(',').skip(1) {
                let mut kv = part.splitn(2, ':');
                if let (Some("adu"), Some(v)) = (kv.next(), kv.next()) {
                    adu = v.parse().ok();
                }
            }

            // 3) Parse each execute-result entry
            let mut results = Vec::new();
            if let Some(list_str) = list_str_opt {
                for entry in list_str.split('|') {
                    let mut slot = None;
                    let mut client_id = None;
                    let mut client_seq = None;
                    let mut op_str = None;
                    let mut success = None;
                    let mut res_str = None;

                    for field in entry.split(',') {
                        let mut kv = field.splitn(2, ':');
                        match (kv.next(), kv.next()) {
                            (Some("slot"), Some(v)) => slot = v.parse().ok(),
                            (Some("client_id"), Some(v)) => client_id = v.parse().ok(),
                            (Some("client_seq"), Some(v)) => client_seq = v.parse().ok(),
                            (Some("op"), Some(v)) => op_str = Some(v.to_string()),
                            (Some("success"), Some(v)) => success = v.parse().ok(),
                            (Some("result"), Some(v)) => res_str = Some(v.to_string()),
                            _ => {}
                        }
                    }

                    if let (Some(s), Some(cid), Some(cseq), Some(op_s), Some(suc), Some(res_s)) =
                        (slot, client_id, client_seq, op_str, success, res_str)
                    {
                        // parse the operation
                        let cmd = if op_s == "none" {
                            None
                        } else {
                            Some(deserialize_operation(&op_s).map_err(|_| "bad op")?)
                        };

                        let val = Value {
                            command: cmd,
                            client_id: cid,
                            client_seq: cseq,
                        };

                        // parse the command result
                        let cmd_res = deserialize_cmd_result(&res_s)?;

                        results.push(ExecuteResult {
                            value: val,
                            slot: s,
                            success: suc,
                            cmd_result: cmd_res,
                        });
                    }
                }
            }

            // 4) Construct the final Executed record
            if let Some(adu) = adu {
                Ok(MessagePayload::Executed(Executed { results, adu }))
            } else {
                Err("bad executed payload")
            }
        }
        "benchmark" => {
            let mut send_timestamp = None;
            let mut payload_str = None;

            for part in &parts[1..] {
                let mut kv = part.splitn(2, ':');
                match (kv.next(), kv.next()) {
                    (Some("send_timestamp"), Some(v)) => send_timestamp = v.parse().ok(),
                    (Some("payload"), Some(v)) => payload_str = Some(v.to_string()),
                    _ => {}
                }
            }

            if let (Some(ts), Some(p)) = (send_timestamp, payload_str) {
                Ok(MessagePayload::Benchmark(
                    crate::bindings::paxos::default::network_types::Benchmark {
                        send_timestamp: ts,
                        payload: p,
                    },
                ))
            } else {
                Err("bad benchmark message")
            }
        }

        "adu" => {
            if parts.len() != 2 {
                return Err("bad adu payload");
            }
            let adu = parts[1].parse::<u64>().map_err(|_| "invalid adu value")?;
            Ok(MessagePayload::Adu(adu))
        }

        _ => Err("unknown payload"),
    }
}

/// Turn an Operation into exactly the string your parser can read back.
fn serialize_operation(op: &Operation) -> String {
    match op {
        Operation::Get(key) => format!("get(\"{}\")", key),
        Operation::Remove(key) => format!("remove(\"{}\")", key),
        Operation::Set(KvPair { key, value }) => {
            format!("set(\"{}\",\"{}\")", key, value)
        }
        Operation::Clear => "clear".into(),
        Operation::Demo => "demo".into(),
    }
}

/// Helper: parse debug-style ops:
///  - get("foo")
///  - set("foo","bar")
///  - remove("foo")
///  - clear
///  - demo
fn deserialize_operation(op_str: &str) -> Result<Operation, &'static str> {
    if op_str == "none" {
        return Err("none");
    }
    // get("key")
    if let Some(arg) = op_str
        .strip_prefix("get(\"")
        .and_then(|s| s.strip_suffix("\")"))
    {
        return Ok(Operation::Get(arg.to_string()));
    }
    // remove("key")
    if let Some(arg) = op_str
        .strip_prefix("remove(\"")
        .and_then(|s| s.strip_suffix("\")"))
    {
        return Ok(Operation::Remove(arg.to_string()));
    }
    // set("key","value")
    if let Some(inner) = op_str
        .strip_prefix("set(")
        .and_then(|s| s.strip_suffix(")"))
    {
        // inner == "\"key\",\"value\""
        if let Some(stripped) = inner.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            let parts: Vec<&str> = stripped.split("\",\"").collect();
            if parts.len() == 2 {
                return Ok(Operation::Set(KvPair {
                    key: parts[0].to_string(),
                    value: parts[1].to_string(),
                }));
            }
        }
    }
    // clear
    if op_str == "clear" {
        return Ok(Operation::Clear);
    }
    // demo
    if op_str == "demo" {
        return Ok(Operation::Demo);
    }
    Err("unknown op")
}
