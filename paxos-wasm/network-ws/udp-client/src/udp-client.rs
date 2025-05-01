use std::net::SocketAddr;

use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::udp::{
    ErrorCode as UdpErrorCode, IncomingDatagramStream, OutgoingDatagram, OutgoingDatagramStream,
    UdpSocket,
};
use wasi::sockets::udp_create_socket::create_udp_socket;

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

/// Create a UDP client socket "connected" to the given full socket address (e.g. "127.0.0.1:8080").
/// For UDP, we bind to an ephemeral local address (0.0.0.0:0) and then use `stream(Some(remote_address))`.
fn create_client_socket(
    socket_addr_str: &str,
) -> Result<(UdpSocket, IncomingDatagramStream, OutgoingDatagramStream), UdpErrorCode> {
    let network = instance_network();
    let socket = create_udp_socket(IpAddressFamily::Ipv4)?;
    let pollable = socket.subscribe();

    // Bind to an ephemeral local address.
    let local_address = IpSocketAddress::Ipv4(Ipv4SocketAddress {
        address: (0, 0, 0, 0),
        port: 0,
    });
    socket.start_bind(&network, local_address)?;
    pollable.block();
    socket.finish_bind()?;

    // Parse the remote (server) address.
    let std_socket_addr: SocketAddr = socket_addr_str
        .parse()
        .expect("Invalid socket address format");
    let remote_address = match std_socket_addr {
        SocketAddr::V4(v4_addr) => {
            let octets = v4_addr.ip().octets();
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                address: (octets[0], octets[1], octets[2], octets[3]),
                port: v4_addr.port(),
            })
        }
        _ => panic!("Expected an IPv4 address"),
    };

    // "Connect" the UDP socket by creating datagram streams scoped to the remote address.
    let (incoming, outgoing) = socket.stream(Some(remote_address))?;
    Ok((socket, incoming, outgoing))
}

/// Our UDP client component (using UDP under the hood).
pub struct TcpClient;

impl Guest for TcpClient {
    /// send-hello simply returns a static greeting.
    fn send_hello() -> String {
        String::from("Hello from TcpClient!")
    }

    /// send-message opens a connection to each node, sends the given message,
    /// waits for a reply, and returns the list of responses.
    fn send_message(nodes: Vec<Node>, message: NetworkMessage) -> Vec<NetworkMessage> {
        let mut responses = Vec::new();
        // Serialize the message using the TCP serializer.
        let msg_bytes = serializer::serialize(&message);
        // For every target node:
        for node in nodes.iter() {
            match create_client_socket(&node.address) {
                Ok((_socket, incoming, outgoing)) => {
                    // Build an OutgoingDatagram.
                    let out_datagram = OutgoingDatagram {
                        data: msg_bytes.clone(),
                        // Since the socket is "connected" via stream(Some(remote_address)),
                        // we set remote_address to None.
                        remote_address: None,
                    };

                    // Check if sending is permitted.
                    match outgoing.check_send() {
                        Ok(permit) if permit > 0 => {
                            if outgoing.send(&[out_datagram]).is_ok() {
                                // Wait briefly for a reply by attempting to receive up to 10 datagrams.
                                if let Ok(datagrams) = incoming.receive(10) {
                                    for datagram in datagrams.into_iter() {
                                        if !datagram.data.is_empty() {
                                            // Deserialize the received bytes.
                                            let resp_msg = serializer::deserialize(&datagram.data);
                                            responses.push(resp_msg);
                                            break; // one reply per node is sufficient
                                        }
                                    }
                                }
                            }
                        }
                        Ok(_) => {
                            logger::log_warn("[UDP Client] No sending permit available");
                        }
                        Err(e) => {
                            logger::log_warn(&format!("[UDP Client] check_send error: {:?}", e));
                        }
                    }
                }
                Err(e) => {
                    logger::log_warn(&format!(
                        "[UDP Client] send_message: Failed to connect to node {}: {:?}",
                        node.address, e
                    ));
                }
            }
        }
        responses
    }

    /// send-message-forget opens a connection to each node, sends the message,
    /// and does not wait for a reply.
    fn send_message_forget(nodes: Vec<Node>, message: NetworkMessage) {
        let msg_bytes = serializer::serialize(&message);
        for node in nodes.iter() {
            logger::log_info(&format!(
                "[UDP Client] send_message_forget: Trying to send message to node {} with message {:?}",
                node.address, message
            ));
            if let Ok((_socket, _incoming, outgoing)) = create_client_socket(&node.address) {
                let out_datagram = OutgoingDatagram {
                    data: msg_bytes.clone(),
                    remote_address: None,
                };
                // Check the send permit before sending.
                match outgoing.check_send() {
                    Ok(permit) if permit > 0 => {
                        if outgoing.send(&[out_datagram]).is_ok() {
                            logger::log_info(&format!(
                                "[UDP Client] send_message_forget: Successfully sent message to node {}",
                                node.address
                            ));
                        } else {
                            logger::log_warn(&format!(
                                "[UDP Client] send_message_forget: Write error to node {}",
                                node.address
                            ));
                        }
                    }
                    Ok(_) => {
                        logger::log_warn(&format!(
                            "[UDP Client] send_message_forget: No sending permit available for node {}",
                            node.address
                        ));
                    }
                    Err(e) => {
                        logger::log_warn(&format!(
                            "[UDP Client] send_message_forget: check_send error for node {}: {:?}",
                            node.address, e
                        ));
                    }
                }
                // Connection is dropped immediately.
            } else {
                logger::log_warn(&format!(
                    "[UDP Client] send_message_forget: Failed to connect to node {}",
                    node.address
                ));
            }
        }
    }
}
