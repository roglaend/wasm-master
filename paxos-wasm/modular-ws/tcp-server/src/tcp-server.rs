use rand::Rng;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;
use wasi::sockets::tcp::InputStream;
use wasi::sockets::tcp::OutputStream;

use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::{ErrorCode as TcpErrorCode, TcpSocket};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "paxos-ws-world",
        additional_derives: [PartialEq, Clone],
    });
}

bindings::export!(MyProposerTcp with_types_in bindings);

use bindings::exports::paxos::default::ws_server::{Guest, GuestWsServerResource, RunConfig};
use bindings::paxos::default::network_types::{MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{ClientResponse, Node, Operation, PaxosRole, Value};
use bindings::paxos::default::{
    acceptor_agent, learner_agent, logger, paxos_coordinator, proposer_agent, serializer,
};

pub struct MyProposerTcp;

impl Guest for MyProposerTcp {
    type WsServerResource = MyTcpServerResource;
}

pub struct Connection {
    socket: TcpSocket,
    // input: InputStream,
    input: Option<InputStream>,
    output: Option<OutputStream>,
}

type RequestKey = (String, u64);

impl Connection {
    pub fn new(socket: TcpSocket, input: InputStream, output: OutputStream) -> Self {
        Self {
            socket,
            input: Some(input),
            output: Some(output),
        }
    }

    pub fn shutdown(mut self) {
        // Take the streams out of the struct so they are dropped here.
        let _ = self.input.take();
        let _ = self.output.take();
        // When self goes out of scope, socket gets dropped after its children.
        // Optionally, you can try to invoke any shutdown on the socket if available.
    }
}

enum Runner {
    Proposer(Arc<proposer_agent::ProposerAgentResource>),
    Acceptor(Arc<acceptor_agent::AcceptorAgentResource>),
    Learner(Arc<learner_agent::LearnerAgentResource>),
    Coordinator(Arc<paxos_coordinator::PaxosCoordinatorResource>),
}

impl Runner {
    /// Delegates the message handling to the appropriate agent.
    fn handle_message(&self, msg: NetworkMessage) -> NetworkMessage {
        match self {
            Runner::Proposer(runner) => runner.handle_message(&msg),
            Runner::Acceptor(runner) => runner.handle_message(&msg),
            Runner::Learner(runner) => runner.handle_message(&msg),
            Runner::Coordinator(runner) => runner.handle_message(&msg),
        }
    }

    fn submit_client_request(&self, value: &Value) -> bool {
        match self {
            Runner::Proposer(runner) => runner.submit_client_request(value),
            Runner::Acceptor(_runner) => false,
            Runner::Learner(_runner) => false,
            Runner::Coordinator(runner) => runner.submit_client_request(value),
        }
    }

    fn is_leader(&self) -> bool {
        match self {
            Runner::Proposer(runner) => runner.is_leader(),
            Runner::Coordinator(runner) => runner.is_leader(),
            _ => false,
        }
    }

    /// Runs the runner’s Paxos loop.
    /// (If a particular role does not need a continuous loop, you can adjust this behavior.)
    fn run_paxos_loop(&self) -> Option<Vec<ClientResponse>> {
        match self {
            Runner::Proposer(runner) => {
                let response = runner.run_paxos_loop();
                if let Some(responses) = response.clone() {
                    for resp in responses {
                        logger::log_info(&format!("[TCP Server] Paxos response: {:?}", resp));
                    }
                }
                response.clone()
            }
            Runner::Acceptor(_runner) => {
                // let _ = runner.run_paxos_loop();
                None
            }
            Runner::Learner(_runner) => {
                // let _ = runner.run_paxos_loop();
                None
            }
            Runner::Coordinator(runner) => {
                let response = runner.run_paxos_loop();
                if let Some(responses) = response.clone() {
                    for resp in responses {
                        logger::log_info(&format!("[TCP Server] Paxos response: {:?}", resp));
                    }
                }
                response.clone()
            }
        }
    }
}

pub struct MyTcpServerResource {
    config: RunConfig,
    node: Node,
    nodes: Vec<Node>, // TODO: might be be useful
    runner: Runner,
    client_connections: RefCell<HashMap<RequestKey, Connection>>,

    last_non_leader_tick: RefCell<Instant>,
    non_leader_interval: Duration,
}

impl MyTcpServerResource {}

impl GuestWsServerResource for MyTcpServerResource {
    fn new(node: Node, nodes: Vec<Node>, is_leader: bool, config: RunConfig) -> Self {
        // Select the appropriate runner based on the node's role.
        let runner = match node.role {
            PaxosRole::Proposer => {
                logger::log_info("[Tcp Server] Initializing as Proposer runner.");
                Runner::Proposer(Arc::new(proposer_agent::ProposerAgentResource::new(
                    &node, &nodes, is_leader, config,
                )))
            }
            PaxosRole::Acceptor => {
                logger::log_info("[Tcp Server] Initializing as Acceptor agent.");
                Runner::Acceptor(Arc::new(acceptor_agent::AcceptorAgentResource::new(
                    &node, &nodes, config,
                )))
            }
            PaxosRole::Learner => {
                logger::log_info("[Tcp Server] Initializing as Learner agent.");
                Runner::Learner(Arc::new(learner_agent::LearnerAgentResource::new(
                    &node, &nodes, config,
                )))
            }
            PaxosRole::Coordinator => {
                logger::log_info("[Tcp Server] Initializing as Coordinator.");
                Runner::Coordinator(Arc::new(paxos_coordinator::PaxosCoordinatorResource::new(
                    &node, &nodes, is_leader, config,
                )))
            }
            // Optionally, handle unexpected roles.
            _ => {
                logger::log_warn("[Tcp Server] Unknown node role; defaulting to Acceptor agent.");
                Runner::Acceptor(Arc::new(acceptor_agent::AcceptorAgentResource::new(
                    &node, &nodes, config,
                )))
            }
        };

        let now = RefCell::new(Instant::now());
        let non_leader_interval = Duration::from_millis(100); // e.g. poll every 100ms off-leader

        logger::log_info(&format!(
            "[TCP Server] Initialized as {} node with {} remote nodes.",
            match node.role {
                PaxosRole::Proposer => "Proposer",
                PaxosRole::Acceptor => "Acceptor",
                PaxosRole::Learner => "Learner",
                _ => "Unknown",
            },
            nodes.len()
        ));
        Self {
            config,
            node,
            nodes,
            runner: runner,
            client_connections: RefCell::new(HashMap::new()),

            last_non_leader_tick: now,
            non_leader_interval,
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

        let mut client_seq: u64 = 0;
        let client_id = "client-1".to_string();
        let interval = 5..10;

        let mut rng = rand::rng();
        let mut next_request_interval = Duration::from_millis(rng.random_range(interval.clone()));
        let mut last_request_time = Instant::now();

        let mut completed_requests = 0;
        let mut last_log = Instant::now();

        sleep(Duration::from_secs(1)); // To make all nodes ready

        loop {
            // Accept incoming TCP messages
            match listener.accept() {
                Ok((socket, input, output)) => {
                    match input.blocking_read(1024) {
                        Ok(buf) if !buf.is_empty() => {
                            let net_msg = serializer::deserialize(&buf);
                            logger::log_info(&format!(
                                "[TCP Server] Received message: {:?}",
                                net_msg
                            ));

                            match net_msg.payload {
                                MessagePayload::ClientRequest(value) => {
                                    if self.runner.submit_client_request(&value) {
                                        let request_key =
                                            (value.client_id.clone(), value.client_seq);

                                        if let Some(connection) = self
                                            .client_connections
                                            .borrow_mut()
                                            .remove(&request_key)
                                        {
                                            connection.shutdown();
                                        }

                                        let connection = Connection::new(socket, input, output);

                                        self.client_connections
                                            .borrow_mut()
                                            .insert(request_key.clone(), connection);
                                        logger::log_info(&format!(
                                            "[TCP Server] Connection inserted successfully. Request Key: {:?}",
                                            request_key
                                        ));
                                    }
                                }
                                _ => {
                                    let _ = self.runner.handle_message(net_msg.clone());
                                }
                            }

                            // TODO
                            // if !self.config.is_event_driven {
                            //     let response_bytes = serializer::serialize(&response_msg);
                            //     if let Err(e) =
                            //         output.blocking_write_and_flush(response_bytes.as_slice())
                            //     {
                            //         logger::log_warn(&format!("[TCP Server] Write error: {:?}", e));
                            //     } else {
                            //         logger::log_info("[TCP Server] Response sent back to client.");
                            //     }
                            // }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            logger::log_warn(&format!("[TCP Server] Read error: {:?}", e));
                        }
                    }
                }
                Err(e) if e == TcpErrorCode::WouldBlock => {
                    let mut maybe_responses = None;

                    if self.runner.is_leader() {
                        // leader runs every tick
                        maybe_responses = self.runner.run_paxos_loop();
                    } else if self.last_non_leader_tick.borrow().elapsed()
                        >= self.non_leader_interval
                    {
                        // non-leaders only once per interval
                        maybe_responses = self.runner.run_paxos_loop();
                        *self.last_non_leader_tick.borrow_mut() = Instant::now();
                    }

                    if self.config.demo_client {
                        // Generate a random client request if the elapsed time exceeds the random interval.
                        if last_request_time.elapsed() > next_request_interval {
                            if self.runner.is_leader() {
                                client_seq += 1;

                                let request = Value {
                                    command: Some(Operation::Demo),
                                    client_id: client_id.clone(),
                                    client_seq,
                                };

                                let submitted = self.runner.submit_client_request(&request);
                                logger::log_info(&format!(
                                    "[TCP Server] Submitted fixed client request seq={} success={}",
                                    client_seq, submitted
                                ));

                                // Reset timer and choose a new random interval.
                                last_request_time = Instant::now();
                                next_request_interval =
                                    Duration::from_millis(rng.random_range(interval.clone()));
                            }
                        }
                    } else {
                        if self.runner.is_leader() {
                            if let Some(responses) = maybe_responses {
                                for response in responses {
                                    let request_key =
                                        (response.client_id.clone(), response.client_seq);
                                    if let Some(connection) =
                                        self.client_connections.borrow_mut().remove(&request_key)
                                    {
                                        let response_msg = NetworkMessage {
                                            sender: self.node.clone(),
                                            payload: MessagePayload::ClientResponse(response),
                                        };
                                        let response_bytes = serializer::serialize(&response_msg);
                                        if let Err(e) = connection
                                            .output
                                            .as_ref()
                                            .unwrap()
                                            .blocking_write_and_flush(response_bytes.as_slice())
                                        {
                                            logger::log_warn(&format!(
                                                "[TCP Server] Write error: {:?}",
                                                e
                                            ));
                                        } else {
                                            completed_requests += 1;
                                            logger::log_info(
                                                "[TCP Server] Response sent back to client.",
                                            );
                                        }
                                        connection.shutdown();
                                    }
                                }
                                if last_log.elapsed() >= Duration::from_millis(500) {
                                    let elapsed_secs = last_log.elapsed().as_secs_f64();
                                    let throughput = (completed_requests as f64) / elapsed_secs;

                                    logger::log_info(&format!(
                                        "[TCP Server] Throughput: {:.2} responses/sec",
                                        throughput
                                    ));

                                    completed_requests = 0;
                                    last_log = Instant::now();
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    logger::log_warn(&format!("[TCP Server] Accept error: {:?}", e));
                }
            }

            // sleep(Duration::from_millis(1));
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
