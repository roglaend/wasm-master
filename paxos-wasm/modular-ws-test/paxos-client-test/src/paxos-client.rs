use std::net::SocketAddr;
use std::thread;
use std::time::Duration;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "paxos-client-world",
        additional_derives: [PartialEq, Clone],
    });
}

// Export our network client component.
bindings::export!(PaxosClient with_types_in bindings);

// Bring in the generated types.
use bindings::exports::paxos::default::paxos_client::Guest;
// use bindings::paxos::default::logger;
use bindings::paxos::default::network_types::{MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{ClientResponse, Node, PaxosRole, Value};
// use bindings::paxos::default::paxos_types::Node;
use bindings::paxos::default::serializer;

// WASI sockets imports.
use wasi::io::streams::{InputStream, OutputStream};
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::{ErrorCode as TcpErrorCode, TcpSocket};
use wasi::sockets::tcp_create_socket::create_tcp_socket;


/// Create a TCP client socket connected to the given full socket address (e.g. "127.0.0.1:8080").
fn create_client_socket(
    socket_addr_str: &str,
) -> Result<(TcpSocket, InputStream, OutputStream), TcpErrorCode> {
    let network = instance_network();
    let socket = create_tcp_socket(IpAddressFamily::Ipv4)?;
    // Parse the full socket address.
    let std_socket_addr: SocketAddr = socket_addr_str
        .parse()
        .expect("Invalid socket address format");

    // Build the remote WASI socket address from the parsed IPv4 address.
    let remote_address = match std_socket_addr {
        SocketAddr::V4(v4_addr) => {
            let octets = v4_addr.ip().octets();
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                address: (octets[0], octets[1], octets[2], octets[3]),
                port: v4_addr.port(),
            })
        }
        _ => panic!("Expected an IPv4 socket address"),
    };

    socket.start_connect(&network, remote_address)?;
    socket.subscribe().block();
    let (input, output) = socket.finish_connect()?;
    Ok((socket, input, output))
}

/// Our TCP client component.
pub struct PaxosClient;



/// Implement the network interface functions.
impl Guest for PaxosClient {
    /// send-hello simply returns a static greeting.
    fn perform_request(leader_address: String, value: Value) -> Option<ClientResponse> {
        let (_socket, input, output) = create_client_socket(&leader_address).unwrap();

        // create network message
        let paxos_role = PaxosRole::Proposer;
        let node = Node {
            node_id: 99,
            address: "123123123123".to_string(),
            role: paxos_role,
        };

        let message = NetworkMessage {
            sender: node,
            payload: MessagePayload::ClientRequest(value)
        };

        let msg_bytes = serializer::serialize(&message);

        if output.write(&msg_bytes).is_ok() {
            match input.blocking_read(1024) {
                Ok(buf) if !buf.is_empty() => {
                    let net_msg = serializer::deserialize(&buf);

                    println!("Received message: {:?}", net_msg);

                    match net_msg.payload {
                        MessagePayload::Executed(client_response) => {
                            Some(client_response)
                        }
                        _ => None,
                    };
                    
                }
                Ok(_) => { return None;}
                Err(e) => {
                    println!("Read error: {:?}", e);
                    return None;
                }
            }
        }
        None
    }
}