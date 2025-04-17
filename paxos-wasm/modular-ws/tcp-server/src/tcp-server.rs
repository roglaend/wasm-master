use bindings::paxos::default::network_types::ClientResponse;
use bindings::paxos::default::network_types::MessagePayload;
use bindings::paxos::default::network_types::NetworkMessage;
use bindings::paxos::default::paxos_types::Value;
use rand::Rng;
use wasi::sockets::tcp::InputStream;
use wasi::sockets::tcp::OutputStream;
use std::cell::Ref;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

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
use bindings::paxos::default::paxos_types::{Node, PaxosRole};
use bindings::paxos::default::{acceptor_agent, learner_agent, logger, proposer_agent, serializer};

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

impl Connection {
    pub fn new(socket: TcpSocket, input: InputStream, output: OutputStream) -> Self {
        Self { 
            socket, 
            input: Some(input), 
            output: Some(output) 
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

enum Agent {
    Proposer(Arc<proposer_agent::ProposerAgentResource>),
    Acceptor(Arc<acceptor_agent::AcceptorAgentResource>),
    Learner(Arc<learner_agent::LearnerAgentResource>),
}

impl Agent {
    /// Delegates the message handling to the appropriate agent.
    fn handle_message(&self, msg: NetworkMessage) -> NetworkMessage {
        match self {
            Agent::Proposer(agent) => agent.handle_message(&msg),
            Agent::Acceptor(agent) => agent.handle_message(&msg),
            Agent::Learner(agent) => agent.handle_message(&msg),
        }
    }

    fn submit_client_request(&self, value: &Value) -> bool {
        match self {
            Agent::Proposer(agent) => agent.submit_client_request(value),
            Agent::Acceptor(_agent) => false,
            Agent::Learner(_agent) => false,
        }
    }

    /// Runs the agent’s Paxos loop.
    /// (If a particular role does not need a continuous loop, you can adjust this behavior.)
    fn run_paxos_loop(&self) -> Option<Vec<ClientResponse>> {
        match self {
            Agent::Proposer(agent) => {
                let response = agent.run_paxos_loop();
                if let Some(responses) = response.clone() {
                    for resp in responses {
                        logger::log_error(&format!("[TCP Server] Paxos response: {:?}", resp));
                    }
                }
                response.clone()
            }
            Agent::Acceptor(_agent) => {
                // let _ = agent.run_paxos_loop();
                None
            }
            Agent::Learner(_agent) => {
                // let _ = agent.run_paxos_loop();
                None
            }
        }
    }
}

pub struct MyTcpServerResource {
    config: RunConfig,
    node: Node,
    nodes: Vec<Node>, // TODO: might be be useful
    agent: Agent,
    client_connections: RefCell<HashMap<u64, Connection>>,
}

impl MyTcpServerResource {}

impl GuestWsServerResource for MyTcpServerResource {
    fn new(node: Node, nodes: Vec<Node>, is_leader: bool, config: RunConfig) -> Self {
        // Select the appropriate agent based on the node's role.
        let agent = match node.role {
            PaxosRole::Proposer => {
                logger::log_info("[Tcp Server] Initializing as Proposer agent.");
                Agent::Proposer(Arc::new(proposer_agent::ProposerAgentResource::new(
                    &node, &nodes, is_leader, config,
                )))
            }
            PaxosRole::Acceptor => {
                logger::log_info("[Tcp Server] Initializing as Acceptor agent.");
                Agent::Acceptor(Arc::new(acceptor_agent::AcceptorAgentResource::new(
                    &node, &nodes, config,
                )))
            }
            PaxosRole::Learner => {
                logger::log_info("[Tcp Server] Initializing as Learner agent.");
                Agent::Learner(Arc::new(learner_agent::LearnerAgentResource::new(
                    &node, &nodes, config,
                )))
            }
            // Optionally, handle unexpected roles.
            _ => {
                logger::log_warn("[Tcp Server] Unknown node role; defaulting to Acceptor agent.");
                Agent::Acceptor(Arc::new(acceptor_agent::AcceptorAgentResource::new(
                    &node, &nodes, config,
                )))
            }
        };

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
            agent,
            client_connections: RefCell::new(HashMap::new()),
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
                        logger::log_info(&format!("[TCP Server] Received message: {:?}", net_msg));

                        

                        match net_msg.payload {
                            MessagePayload::ClientRequest(value) => {

                                if self.agent.submit_client_request(&value) {
                                    let request_id = value.client_id * 1000 + value.client_seq;

                                    if let Some(connection) = self.client_connections.borrow_mut().remove(&request_id) {
                                        connection.shutdown();
                                    }

                                    let connection = Connection::new(socket, input, output);
    
                                    self.client_connections.borrow_mut().insert(request_id, connection);
                                }
                            }
                            _ => { 
                                let _ = self.agent.handle_message(net_msg.clone());
                            }
                        }

                            

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

                let resp = self.agent.run_paxos_loop();

                if self.config.demo_client {
                    // Generate a random client request if the elapsed time exceeds the random interval.
                    if last_request_time.elapsed() > next_request_interval {
                        if let Agent::Proposer(agent) = &self.agent {
                            if agent.is_leader() {
                                client_seq += 1;
    
                                let request = Value {
                                    is_noop: false,
                                    command: Some("cmd".to_string()), // Use a constant command
                                    client_id: 1,                     // consistent ID as u64 for Value
                                    client_seq,
                                };
    
                                let submitted = agent.submit_client_request(&request);
                                logger::log_info(&format!(
                                    "[TCP Server] Submitted fixed client request seq={} success={}",
                                    client_seq, submitted
                                ));
                            }
    
                            // Reset the timer and pick a new random interval between 0.5 and 1.5 seconds.
                            last_request_time = Instant::now();
                            next_request_interval =
                                Duration::from_millis(rng.random_range(interval.clone()));
                        }
                    }
                } else {
                    // If not in demo mode, run the agent's Paxos loop.
                    if let Agent::Proposer(agent) = &self.agent {
                        if agent.is_leader() {
                            if let Some(responses) = resp {
                                for response in responses {
                                    let client_id: u64 = response.client_id.clone().parse().unwrap_or(0);
                                    let request_id = client_id * 1000 + response.client_seq;
                                    if let Some(connection) = self.client_connections.borrow_mut().remove(&request_id) {
                                        let response_msg = NetworkMessage {
                                            sender: self.node.clone(),
                                            payload: MessagePayload::Executed(response),
                                        };
                                        let response_bytes = serializer::serialize(&response_msg);
                                        if let Err(e) = connection.output.as_ref().unwrap().blocking_write_and_flush(response_bytes.as_slice()) {
                                            logger::log_warn(&format!("[TCP Server] Write error: {:?}", e));
                                        } else {
                                            completed_requests += 1;
                                            logger::log_info("[TCP Server] Response sent back to client.");
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
