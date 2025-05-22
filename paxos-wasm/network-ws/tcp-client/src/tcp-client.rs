use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;

use wasi::io::streams::{InputStream, OutputStream, StreamError};
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::{ErrorCode as TcpErrorCode, TcpSocket};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "network-client-world",
        additional_derives: [PartialEq, Clone],
    });
}
bindings::export!(TcpClient with_types_in bindings);

use bindings::exports::paxos::default::network_client::{Guest, GuestNetworkClientResource};
use bindings::paxos::default::network_types::NetworkMessage;
use bindings::paxos::default::paxos_types::Node;
use bindings::paxos::default::{logger, serializer};

/// Simple struct to hold our streams and keep the socket alive
struct Connection {
    socket: TcpSocket,
    input: InputStream,
    output: OutputStream,
}

/// The stateful resource: one Connection + buffer per peer address
struct TcpClientResource {
    conns: RefCell<HashMap<String, Connection>>,
    bufs: RefCell<HashMap<String, Vec<u8>>>,
}

impl TcpClientResource {
    /// If we don’t yet have a conn to `addr`, open one and insert an empty buffer.
    fn ensure_conn(&self, addr: &str) -> Result<(), TcpErrorCode> {
        if !self.conns.borrow().contains_key(addr) {
            let (socket, input, output) = create_client_socket(addr)?;
            self.conns.borrow_mut().insert(
                addr.to_string(),
                Connection {
                    socket,
                    input,
                    output,
                },
            );
            self.bufs.borrow_mut().insert(addr.to_string(), Vec::new());
        }
        Ok(())
    }
}

/// Our TCP client component.
struct TcpClient;

impl Guest for TcpClient {
    type NetworkClientResource = TcpClientResource;
}

impl GuestNetworkClientResource for TcpClientResource {
    fn new() -> Self {
        Self {
            conns: RefCell::new(HashMap::new()),
            bufs: RefCell::new(HashMap::new()),
        }
    }

    // TODO
    fn send_message(&self, nodes: Vec<Node>, message: NetworkMessage) -> Vec<NetworkMessage> {
        let msg_bytes = serializer::serialize(&message);
        let mut replies = Vec::new();

        // 1) Send on each connection (creating it if needed)
        for node in &nodes {
            let addr = &node.address;
            if let Err(e) = self.ensure_conn(addr) {
                logger::log_warn(&format!("[TCP Client] connect {} failed: {:?}", addr, e));
                continue;
            }
            let mut conns = self.conns.borrow_mut();
            let conn = conns.get_mut(addr).unwrap();
            if conn.output.blocking_write_and_flush(&msg_bytes).is_err() {
                logger::log_warn(&format!("[TCP Client] write to {} failed", addr));
            }
        }

        // 2) Read from each connection and frame‐decode
        let mut conns = self.conns.borrow_mut();
        let mut bufs = self.bufs.borrow_mut();
        for node in &nodes {
            let addr = &node.address;
            if let Some(conn) = conns.get_mut(addr) {
                if let Ok(chunk) = conn.input.read(65536) {
                    if !chunk.is_empty() {
                        let buf = bufs.get_mut(addr).unwrap();
                        buf.extend_from_slice(&chunk);

                        // pull out all complete 4-byte-len prefixed frames
                        let mut offset = 0;
                        while buf.len() >= offset + 4 {
                            let len =
                                u32::from_be_bytes(buf[offset..offset + 4].try_into().unwrap())
                                    as usize;
                            if buf.len() < offset + 4 + len {
                                break;
                            }

                            // reattach header so our serializer can parse
                            let mut frame = (len as u32).to_be_bytes().to_vec();
                            frame.extend_from_slice(&buf[offset + 4..offset + 4 + len]);
                            offset += 4 + len;

                            let msg: NetworkMessage = serializer::deserialize(&frame);
                            replies.push(msg);
                        }

                        if offset > 0 {
                            buf.drain(0..offset);
                        }
                    }
                }
            }
        }

        replies
    }

    fn send_message_forget(&self, nodes: Vec<Node>, message: NetworkMessage) {
        let msg_bytes = serializer::serialize(&message);

        for node in &nodes {
            let addr = &node.address;

            if let Err(e) = self.ensure_conn(addr) {
                logger::log_warn(&format!("[TCP Client] connect {} failed: {:?}", addr, e));
                continue;
            }

            let mut remove = false;
            {
                let mut conns = self.conns.borrow_mut();
                if let Some(conn) = conns.get_mut(addr) {
                    match conn.output.blocking_write_and_flush(&msg_bytes) {
                        Ok(()) => {}

                        Err(StreamError::Closed) => {
                            logger::log_error(&format!(
                                "[TCP Client] stream closed on {}; dropping connection",
                                addr
                            ));
                            remove = true;
                        }

                        Err(StreamError::LastOperationFailed(err)) => {
                            logger::log_error(&format!(
                                "[TCP Client] write+flush error on {}: {:?}; dropping connection",
                                addr, err
                            ));
                            remove = true;
                        }
                    }
                }
            }

            // Evict the broken connection so next time we reconnect
            if remove {
                if let Some(conn) = self.conns.borrow_mut().remove(addr) {
                    drop(conn.input);
                    drop(conn.output);
                    drop(conn.socket);
                }
                self.bufs.borrow_mut().remove(addr);
            }
        }
    }
}

fn create_client_socket(
    socket_addr_str: &str,
) -> Result<(TcpSocket, InputStream, OutputStream), TcpErrorCode> {
    let network = instance_network();
    let socket = create_tcp_socket(IpAddressFamily::Ipv4)?;
    let std_addr: SocketAddr = socket_addr_str.parse().expect("invalid address");

    let remote = if let SocketAddr::V4(v4) = std_addr {
        let oct = v4.ip().octets();
        IpSocketAddress::Ipv4(Ipv4SocketAddress {
            address: (oct[0], oct[1], oct[2], oct[3]),
            port: v4.port(),
        })
    } else {
        panic!("only IPv4 supported");
    };

    socket.start_connect(&network, remote)?;
    socket.subscribe().block();
    let (input, output) = socket.finish_connect()?;
    Ok((socket, input, output))
}
