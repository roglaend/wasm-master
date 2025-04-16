use bindings::paxos::default::network_types::NetworkMessage;
use bindings::paxos::default::paxos_types::ClientRequest;
use bindings::paxos::default::paxos_types::Value;
use rand::Rng;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::udp::{ErrorCode as UdpErrorCode, OutgoingDatagram, UdpSocket};
use wasi::sockets::udp_create_socket::create_udp_socket;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "paxos-ws-world",
        additional_derives: [PartialEq, Clone],
    });
}

bindings::export!(MyTcpServer with_types_in bindings);

use bindings::exports::paxos::default::ws_server::{Guest, GuestWsServerResource, RunConfig};
use bindings::paxos::default::paxos_types::{Node, PaxosRole};
use bindings::paxos::default::{acceptor_agent, learner_agent, logger, proposer_agent, serializer};

pub struct MyTcpServer;

impl Guest for MyTcpServer {
    type WsServerResource = MyTcpServerResource;
}

enum Agent {
    Proposer(Arc<proposer_agent::ProposerAgentResource>),
    Acceptor(Arc<acceptor_agent::AcceptorAgentResource>),
    Learner(Arc<learner_agent::LearnerAgentResource>),
}

impl Agent {
    /// Delegates message handling to the appropriate Paxos agent.
    fn handle_message(&self, msg: NetworkMessage) -> NetworkMessage {
        match self {
            Agent::Proposer(agent) => agent.handle_message(&msg),
            Agent::Acceptor(agent) => agent.handle_message(&msg),
            Agent::Learner(agent) => agent.handle_message(&msg),
        }
    }

    /// Runs the Paxos algorithm loop if needed.
    fn run_paxos_loop(&self) {
        match self {
            Agent::Proposer(agent) => {
                if let Some(responses) = agent.run_paxos_loop() {
                    for resp in responses {
                        logger::log_error(&format!("[UDP Server] Paxos response: {:?}", resp));
                    }
                }
            }
            Agent::Acceptor(_agent) => { /* No continuous loop needed for Acceptor */ }
            Agent::Learner(_agent) => {
                /* No continuous loop needed for Learner */
                // let _ = agent.run_paxos_loop();
            }
        }
    }
}

pub struct MyTcpServerResource {
    config: RunConfig,
    node: Node,
    nodes: Vec<Node>, // List of remote Paxos nodes.
    agent: Agent,
    // We now store only the UdpSocket.
    socket: UdpSocket,
}

impl MyTcpServerResource {}

impl GuestWsServerResource for MyTcpServerResource {
    fn new(node: Node, nodes: Vec<Node>, is_leader: bool, config: RunConfig) -> Self {
        // Initialize the agent based on the node's role.
        let agent = match node.role {
            PaxosRole::Proposer => {
                logger::log_info("[UDP Server] Initializing as Proposer agent.");
                Agent::Proposer(Arc::new(proposer_agent::ProposerAgentResource::new(
                    &node, &nodes, is_leader, config,
                )))
            }
            PaxosRole::Acceptor => {
                logger::log_info("[UDP Server] Initializing as Acceptor agent.");
                Agent::Acceptor(Arc::new(acceptor_agent::AcceptorAgentResource::new(
                    &node, &nodes, config,
                )))
            }
            PaxosRole::Learner => {
                logger::log_info("[UDP Server] Initializing as Learner agent.");
                Agent::Learner(Arc::new(learner_agent::LearnerAgentResource::new(
                    &node, &nodes, config,
                )))
            }
            _ => {
                logger::log_warn("[UDP Server] Unknown node role; defaulting to Acceptor agent.");
                Agent::Acceptor(Arc::new(acceptor_agent::AcceptorAgentResource::new(
                    &node, &nodes, config,
                )))
            }
        };

        logger::log_info(&format!(
            "[UDP Server] Initialized as {} node with {} remote nodes.",
            match node.role {
                PaxosRole::Proposer => "Proposer",
                PaxosRole::Acceptor => "Acceptor",
                PaxosRole::Learner => "Learner",
                _ => "Unknown",
            },
            nodes.len()
        ));

        // Create and bind the UDP socket.
        let udp_socket = create_udp_listener(&node.address).expect("Failed to create UDP listener");

        Self {
            config,
            node,
            nodes,
            agent,
            socket: udp_socket,
        }
    }

    fn run(&self) {
        logger::log_info(&format!(
            "[UDP Server] Listening on address: {:?}",
            self.node.address,
        ));

        let mut client_seq: u64 = 0;
        let client_id = "client-1".to_string();
        let interval = 5..10;

        let mut rng = rand::rng();
        let mut next_request_interval = Duration::from_millis(rng.random_range(interval.clone()));
        let mut last_request_time = Instant::now();

        // Delay to allow all nodes to be ready.
        sleep(Duration::from_secs(1));

        loop {
            // Each loop iteration we obtain new datagram streams from the UDP socket.
            // This is required since we cannot store the streams permanently.
            if let Ok((incoming, outgoing)) = self.socket.stream(None) {
                // Attempt to receive up to 10 datagrams.
                if let Ok(datagrams) = incoming.receive(10) {
                    for datagram in datagrams.into_iter() {
                        if !datagram.data.is_empty() {
                            // Deserialize using the tcp_serializer (kept for TCP naming).
                            let net_msg = serializer::deserialize(&datagram.data);
                            logger::log_info(&format!(
                                "[UDP Server] Received message from {:?}: {:?}",
                                datagram.remote_address, net_msg
                            ));

                            let response_msg = self.agent.handle_message(net_msg);

                            if !self.config.is_event_driven {
                                let response_bytes = serializer::serialize(&response_msg);
                                // Build an OutgoingDatagram targeted to the sender.
                                let out_datagram = OutgoingDatagram {
                                    data: response_bytes,
                                    remote_address: Some(datagram.remote_address),
                                };
                                match outgoing.send(&[out_datagram]) {
                                    Ok(num_sent) if num_sent > 0 => {
                                        logger::log_info(
                                            "[UDP Server] Response sent back to client.",
                                        );
                                    }
                                    Ok(_) => {
                                        logger::log_warn("[UDP Server] No datagrams sent.");
                                    }
                                    Err(e) => {
                                        logger::log_warn(&format!(
                                            "[UDP Server] Write error: {:?}",
                                            e
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                // Streams go out of scope and are dropped here.
            }

            if (self.config.demo_client) {
                // Periodically submit a client request if enough time has elapsed.
                if last_request_time.elapsed() > next_request_interval {
                    if let Agent::Proposer(agent) = &self.agent {
                        if agent.is_leader() {
                            // for _ in 0..10 {
                            client_seq += 1;
                            let cmd_value = Value {
                                is_noop: false,
                                command: Some("cmd".to_string()),
                                client_id: 1,
                                client_seq,
                            };

                            let request = ClientRequest {
                                client_id: client_id.clone(),
                                client_seq,
                                value: cmd_value,
                            };

                            let submitted = agent.submit_client_request(&request);
                            logger::log_info(&format!(
                                "[UDP Server] Submitted fixed client request seq={} success={}",
                                client_seq, submitted
                            ));
                        }
                        // }
                        // Reset timer and choose a new random interval.
                        last_request_time = Instant::now();
                        next_request_interval =
                            Duration::from_millis(rng.random_range(interval.clone()));
                    }
                }
            }

            self.agent.run_paxos_loop();
            sleep(Duration::from_millis(1));
        }
    }
}

/// Creates a UDP socket, binds it to the supplied address (for example, "127.0.0.1:8080"),
/// and returns the bound socket.
/// With UDP you do not “listen” or “accept” connections.
fn create_udp_listener(bind_address: &str) -> Result<UdpSocket, UdpErrorCode> {
    let network = instance_network();
    let socket = create_udp_socket(IpAddressFamily::Ipv4)?;
    let pollable = socket.subscribe();

    // Parse the bind address (e.g. "127.0.0.1:8080").
    let std_socket_addr: SocketAddr = bind_address
        .parse()
        .expect("Invalid socket address format in node");

    // Convert into WASI’s IpSocketAddress.
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

    Ok(socket)
}
