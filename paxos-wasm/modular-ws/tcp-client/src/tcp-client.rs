use std::net::SocketAddr;

use wasi::io::streams::{InputStream, OutputStream};
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::{ErrorCode as TcpErrorCode, TcpSocket};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "ws-client-world",
        additional_derives: [PartialEq, Clone],
    });
}

bindings::export!(TcpClient with_types_in bindings);

use bindings::exports::paxos::default::network::Guest;
use bindings::paxos::default::logger;
use bindings::paxos::default::network_types::NetworkMessage;
use bindings::paxos::default::paxos_types::Node;
use bindings::paxos::default::serializer;

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
pub struct TcpClient;

/// Implement the network interface functions.
impl Guest for TcpClient {
    /// send-hello simply returns a static greeting.
    fn send_hello() -> String {
        String::from("Hello from TcpClient!")
    }

    /// send-message opens a connection to each node, sends the given message,
    /// waits for a reply, and returns the list of responses.
    fn send_message(nodes: Vec<Node>, message: NetworkMessage) -> Vec<NetworkMessage> {
        let mut responses = Vec::new();
        // Use the shared serializer to convert the NetworkMessage into bytes.
        let msg_bytes = serializer::serialize(&message);
        // For every target node:
        for node in nodes.iter() {
            match create_client_socket(&node.address) {
                Ok((_socket, input, output)) => {
                    // Send the message bytes.
                    if output.blocking_write_and_flush(&msg_bytes).is_ok() {
                        // Wait briefly for a reply. // TODO: lol?
                        if let Ok(buf) = input.read(1024) {
                            if !buf.is_empty() {
                                // Use the shared serializer to convert the received bytes back to a NetworkMessage.
                                let resp_msg = serializer::deserialize(&buf);
                                responses.push(resp_msg);
                            }
                        }
                    }
                }
                Err(e) => {
                    logger::log_warn(&format!(
                        "[TCP Client] send_message: Failed to connect to node {}: {:?}",
                        node.address, e
                    ));
                }
            }
        }
        responses
    }

    /// send-message-forget opens a connection to each node, sends the message, and does not wait for a reply.
    fn send_message_forget(nodes: Vec<Node>, message: NetworkMessage) {
        let msg_bytes = serializer::serialize(&message);
        for node in nodes.iter() {
            logger::log_info(&format!(
                "[TCP Client] send_message_forget: Trying to send message to node {} with message {:?}",
                node.address, message
            ));
            if let Ok((_socket, _input, output)) = create_client_socket(&node.address) {
                if output.blocking_write_and_flush(&msg_bytes).is_ok() {
                    logger::log_info(&format!(
                        "[TCP Client] send_message_forget: Successfully sent message to node {}",
                        node.address
                    ));
                } else {
                    logger::log_warn(&format!(
                        "[TCP Client] send_message_forget: Write error to node {}",
                        node.address
                    ));
                }
                // Immediately dropping the connection.
            } else {
                logger::log_warn(&format!(
                    "[TCP Client] send_message_forget: Failed to connect to node {}",
                    node.address
                ));
            }
        }
    }
}
