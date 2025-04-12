use crate::{coordinator_bindings, wit_convert, wit_convert_enum_unit};

//
// Aliases for paxos-types
//
use coordinator_bindings::paxos::default::paxos_types as Host;
use paxos_wasm_bindings_types::exports::paxos::default::paxos_types as Shared;

//
// Aliases for network-types (if they live in a separate module)
use coordinator_bindings::paxos::default::network_types as HostNetwork;
use paxos_wasm_bindings_types::exports::paxos::default::network_types as SharedNetwork;

// ---------------------------------------------------------------------
// Implement conversions for enums that carry data – for example, MessagePayload.
// (Both sets of types are assumed to be PascalCase.)
impl From<HostNetwork::MessagePayload> for SharedNetwork::MessagePayload {
    fn from(value: HostNetwork::MessagePayload) -> Self {
        use HostNetwork::MessagePayload as H;
        use SharedNetwork::MessagePayload as S;
        match value {
            H::Ignore => S::Ignore,
            H::Prepare(payload) => S::Prepare(payload.into()),
            H::Promise(payload) => S::Promise(payload.into()),
            H::Accept(payload) => S::Accept(payload.into()),
            H::Accepted(payload) => S::Accepted(payload.into()),
            H::Learn(payload) => S::Learn(payload.into()),
            H::Heartbeat(payload) => S::Heartbeat(payload.into()),
            H::RetryLearn(payload) => S::RetryLearn(payload.into()),
            H::Executed(payload) => S::Executed(payload.into()),
        }
    }
}

impl From<SharedNetwork::MessagePayload> for HostNetwork::MessagePayload {
    fn from(value: SharedNetwork::MessagePayload) -> Self {
        use HostNetwork::MessagePayload as H;
        use SharedNetwork::MessagePayload as S;
        match value {
            S::Ignore => H::Ignore,
            S::Prepare(payload) => H::Prepare(payload.into()),
            S::Promise(payload) => H::Promise(payload.into()),
            S::Accept(payload) => H::Accept(payload.into()),
            S::Accepted(payload) => H::Accepted(payload.into()),
            S::Learn(payload) => H::Learn(payload.into()),
            S::Heartbeat(payload) => H::Heartbeat(payload.into()),
            S::RetryLearn(payload) => H::RetryLearn(payload.into()),
            S::Executed(payload) => H::Executed(payload.into()),
        }
    }
}

// ---------------------------------------------------------------------
// Use the macros for record and unit enum conversions between Host and Shared.

// Convert NetworkMessage (assumed to be a record with fields `sender` and `payload`)
wit_convert! {
    HostNetwork::NetworkMessage => SharedNetwork::NetworkMessage { sender, payload }
}

// Convert Node (a record: Node { node_id, address, role }).
wit_convert! {
    Host::Node => Shared::Node { node_id, address, role }
}

// Convert PaxosRole (a unit enum with variants in PascalCase)
wit_convert_enum_unit! {
    Host::PaxosRole => Shared::PaxosRole { Coordinator, Proposer, Acceptor, Learner }
}

// ---------------------------------------------------------------------
// Additional conversions for the remaining paxos-types:

// RunConfig: record { is_event_driven, acceptors_send_learns, prepare_timeout }
wit_convert! {
    Host::RunConfig => Shared::RunConfig { is_event_driven, acceptors_send_learns, prepare_timeout }
}

// PaxosPhase: unit enum
wit_convert_enum_unit! {
    Host::PaxosPhase => Shared::PaxosPhase { Start, PrepareSend, PreparePending, AcceptCommit, Stop, Crash }
}

// For other records, follow the same pattern:
wit_convert! {
    Host::Value => Shared::Value { is_noop, command, client_id, client_seq }
}

wit_convert! {
    Host::ClientRequest => Shared::ClientRequest { client_id, client_seq, value }
}

wit_convert! {
    Host::ClientResponse => Shared::ClientResponse { client_id, client_seq, success, command_result, slot }
}

wit_convert! {
    Host::Proposal => Shared::Proposal { slot, ballot, value }
}

wit_convert! {
    Host::Prepare => Shared::Prepare { slot, ballot }
}

wit_convert! {
    Host::Promise => Shared::Promise { slot, ballot, accepted }
}

wit_convert! {
    Host::PValue => Shared::PValue { slot, ballot, value }
}

wit_convert! {
    Host::Accept => Shared::Accept { slot, ballot, value }
}

wit_convert! {
    Host::Accepted => Shared::Accepted { slot, ballot, success }
}

wit_convert! {
    Host::Learn => Shared::Learn { slot, value }
}

wit_convert! {
    HostNetwork::Heartbeat => SharedNetwork::Heartbeat { sender, timestamp }
}
