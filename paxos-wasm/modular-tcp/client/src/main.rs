pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "command-paxos-tcp-world",
        additional_derives: [PartialEq, Clone],
    });
}

use bindings::paxos::default::paxos_tcp_client::perform_request;
use bindings::paxos::default::network_types::{
    MessagePayload, NetworkMessage,
};
use bindings::paxos::default::paxos_types::{Node, PaxosRole};

fn main() {
    println!("Hello, world!");
    let paxos_role = PaxosRole::Proposer;

    let node = Node {
        node_id: 99,
        address: "123123123123".to_string(),
        role: paxos_role,
    };

    let client_request = MessagePayload::ClientRequest(
        bindings::paxos::default::paxos_types::Value {
            is_noop: false,
            command: Some("set foo bar".to_string()),
            client_id: 2,
            client_seq: 1,

        },
    );

    let network_message = NetworkMessage {
        sender: node.clone(),
        payload: client_request,
    };

    if let Some(res) = perform_request(&network_message) {
        println!("Response: {:?}", res);
    } else {
        println!("No response received.");
    }

}