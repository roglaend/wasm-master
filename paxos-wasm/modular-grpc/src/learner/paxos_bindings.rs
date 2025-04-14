// TODO: Create a shared package for this
wasmtime::component::bindgen! {{
    path: "../shared/wit",
    world: "learner-agent-world",
    additional_derives: [Clone],
    async: true,
    // TODO: Try async again later
}}

use crate::learner::paxos_bindings::paxos::default::network_types::MessagePayload;

// Helper function to get a string conversion for MessagePayload without considering content.
pub trait MessagePayloadExt {
    fn payload_type(&self) -> &'static str;
}

impl MessagePayloadExt for MessagePayload {
    fn payload_type(&self) -> &'static str {
        match self {
            MessagePayload::Prepare(_) => "Prepare",
            MessagePayload::Promise(_) => "Promise",
            MessagePayload::Accept(_) => "Accept",
            MessagePayload::Accepted(_) => "Accepted",
            MessagePayload::Learn(_) => "Learn",
            MessagePayload::Heartbeat(_) => "Heartbeat",
            MessagePayload::Ignore => "Ignore",
            MessagePayload::RetryLearn(_) => "RetryLearn",
            MessagePayload::Executed(_) => "Executed",
            _ => "Unknown",
        }
    }
}