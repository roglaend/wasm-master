use std::net::SocketAddr;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::{ErrorCode as TcpErrorCode, TcpSocket};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "paxos-tcp-world",
        additional_derives: [PartialEq, Clone],
    });
}

bindings::export!(MyProposerTcp with_types_in bindings);

use bindings::exports::paxos::default::tcp_server::{Guest, GuestTcpServerResource, RunConfig};
use bindings::paxos::default::paxos_types::{Node, PaxosRole};
use bindings::paxos::default::{
    acceptor_agent, learner_agent, logger, proposer_agent, tcp_serializer,
};

pub struct MyProposerTcp;

impl Guest for MyProposerTcp {
    type TcpServerResource = MyTcpServerResource;
}

pub struct MyTcpServerResource {
    config: RunConfig,
    node: Node,
    nodes: Vec<Node>, // TODO: might be be useful
    proposer_agent: Arc<proposer_agent::ProposerAgentResource>,
    acceptor_agent: Arc<acceptor_agent::AcceptorAgentResource>,
    learner_agent: Arc<learner_agent::LearnerAgentResource>,
}

impl MyTcpServerResource {}

impl GuestTcpServerResource for MyTcpServerResource {
    fn new(node: Node, nodes: Vec<Node>, is_leader: bool, config: RunConfig) -> Self {
        let proposer_agent: Arc<proposer_agent::ProposerAgentResource> = Arc::new(
            proposer_agent::ProposerAgentResource::new(&node, &nodes, is_leader, config),
        );

        let acceptor_agent = Arc::new(acceptor_agent::AcceptorAgentResource::new(
            &node, &nodes, config,
        ));

        let learner_agent = Arc::new(learner_agent::LearnerAgentResource::new(
            &node, &nodes, config,
        ));

        logger::log_info(&format!(
            "[Core Proposer] Initialized as {} node with {} remote nodes.",
            if is_leader { "leader" } else { "normal" },
            nodes.len()
        ));
        Self {
            config,
            node,
            nodes: nodes,
            proposer_agent,
            acceptor_agent,
            learner_agent,
        }
    }

    // TODO: Also add logic for heartbeats, failure service etc.

    fn run(&self) {
        // Use the node's full socket address (e.g., "127.0.0.1:8080") as the bind address.
        let listener = create_listener(&self.node.address).expect("Failed to create TCP listener");

        logger::log_info(&format!(
            "[TCP Server] Listening on address: {:?}",
            self.node.address,
        ));
        loop {
            // Accept a connection, if available (non-blocking).
            if let Ok((_, input, output)) = listener.accept() {
                match input.read(1024) {
                    Ok(buf) if !buf.is_empty() => {
                        let net_msg = tcp_serializer::deserialize(&buf);
                        logger::log_info(&format!("[TCP Server] Received message: {:?}", net_msg));

                        // Delegate handling to the appropriate agent based on the node's role.
                        let response_msg = match self.node.role {
                            PaxosRole::Proposer => self.proposer_agent.handle_message(&net_msg),
                            PaxosRole::Acceptor => self.acceptor_agent.handle_message(&net_msg),
                            PaxosRole::Learner => self.learner_agent.handle_message(&net_msg),
                            // For any unknown role, log a warning and continue.
                            _ => {
                                logger::log_warn(
                                    "[TCP Server] Unknown node role; message not handled.",
                                );
                                continue;
                            }
                        };
                        if !self.config.is_event_driven {
                            // Serialize the response.
                            let response_bytes = tcp_serializer::serialize(&response_msg);

                            // Write the response to the output stream.
                            if let Err(e) = output.write(response_bytes.as_slice()) {
                                logger::log_warn(&format!("[TCP Server] Write error: {:?}", e));
                            } else {
                                logger::log_info("[TCP Server] Response sent back to client.");
                            }
                        }
                    }
                    Ok(_) => { /* No data received */ }
                    Err(e) => {
                        logger::log_warn(&format!("[TCP Server] Read error: {:?}", e));
                    }
                }
            }
            // Tick the Paxos loop by delegating to the appropriate agent.
            match self.node.role {
                PaxosRole::Proposer => {
                    let _ = self.proposer_agent.run_paxos_loop();
                }
                // PaxosRole::Acceptor => {
                //     let _ = self.acceptor_agent.run_paxos_loop();
                // }
                PaxosRole::Learner => {
                    let _ = self.learner_agent.run_paxos_loop();
                }
                _ => {
                    // Unknown roles can be silently skipped or logged.
                    // logger::log_warn("[TCP Server] Unknown node role during Paxos loop tick.");
                }
            }
            sleep(Duration::from_millis(10)); // TODO
        }
    }
}

/// The provided `bind_address` should be a full socket address, e.g. "127.0.0.1:8080".
fn create_listener(bind_address: &str) -> Result<TcpSocket, TcpErrorCode> {
    let network = instance_network();
    let socket = create_tcp_socket(IpAddressFamily::Ipv4)?;
    // Subscribe to a pollable so we can wait for binding and listening.
    let pollable = socket.subscribe();

    // Parse the full socket address.
    let std_socket_addr: SocketAddr = bind_address
        .parse()
        .expect("Invalid socket address format in node");

    // Ensure we have an IPv4 address.
    let local_address = match std_socket_addr {
        SocketAddr::V4(v4_addr) => {
            let octets = v4_addr.ip().octets();
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                address: (octets[0], octets[1], octets[2], octets[3]),
                port: v4_addr.port(),
            })
        }
        _ => panic!("Expected an IPv4 address"),
    };

    socket.start_bind(&network, local_address)?;
    pollable.block();
    socket.finish_bind()?;

    socket.start_listen()?;
    pollable.block();
    socket.finish_listen()?;

    Ok(socket)
}
