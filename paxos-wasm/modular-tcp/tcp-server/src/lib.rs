use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use bindings::paxos::default::network::NetworkMessage;
use bindings::paxos::default::network_types::MessagePayload;
use wasi::io::streams::OutputStream;
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::{ErrorCode as TcpErrorCode, InputStream, TcpSocket};
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
use bindings::paxos::default::paxos_types::{Node, PaxosRole, Value};
use bindings::paxos::default::{
    acceptor_agent, learner_agent, logger, proposer_agent, tcp_serializer,
};

pub struct MyProposerTcp;

impl Guest for MyProposerTcp {
    type TcpServerResource = MyTcpServerResource;
}

pub enum Agent {
    Proposer(Arc<proposer_agent::ProposerAgentResource>),
    Acceptor(Arc<acceptor_agent::AcceptorAgentResource>),
    Learner(Arc<learner_agent::LearnerAgentResource>),
    Coordinator()
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


pub struct MyTcpServerResource {
    config: RunConfig,
    node: Node,
    nodes: Vec<Node>, // TODO: might be be useful
    
    paxos_agent: Agent,

    connections: RefCell<HashMap<u64, Connection>>,

    // proposer_agent: Option<Arc<proposer_agent::ProposerAgentResource>>,
    // acceptor_agent: Option<Arc<acceptor_agent::AcceptorAgentResource>>,
    // learner_agent: Option<Arc<learner_agent::LearnerAgentResource>>,
}

impl MyTcpServerResource {}

impl GuestTcpServerResource for MyTcpServerResource {
    fn new(node: Node, nodes: Vec<Node>, is_leader: bool, config: RunConfig) -> Self {
        let paxos_agent = match node.role {
            PaxosRole::Proposer => {
                let proposer_agent: Arc<proposer_agent::ProposerAgentResource> = Arc::new(
                        proposer_agent::ProposerAgentResource::new(&node, &nodes, is_leader, config),
                    );
                let paxos_agent = Agent::Proposer(proposer_agent.clone());
                paxos_agent
            }
            PaxosRole::Acceptor => {
                let acceptor_agent = Arc::new(acceptor_agent::AcceptorAgentResource::new(
                    &node, &nodes, config,
                ));
                let paxos_agent = Agent::Acceptor(acceptor_agent.clone());
                paxos_agent
            }
            PaxosRole::Learner => {
                let learner_agent = Arc::new(learner_agent::LearnerAgentResource::new(
                    &node, &nodes, config,
                ));
                let paxos_agent = Agent::Learner(learner_agent.clone());
                paxos_agent
            }
            _ => {

                // TODO : Handle coordinator node role. This would also mean a different run function to run coordinaoor because of
                // all of the local calls
                logger::log_warn(&format!(
                    "[Core Proposer] Not supported node role: {:?}.",
                    node.role
                ));

                let paxos_agent = Agent::Coordinator();
                paxos_agent
            }
        };



        logger::log_info(&format!(
            "[Core Proposer] Initialized as {} node with {} remote nodes.",
            if is_leader { "leader" } else { "normal" },
            nodes.len()
        ));
        Self {
            config,
            node,
            nodes,
            paxos_agent,
            connections: RefCell::new(HashMap::new()),
        }
    }

    // TODO: Also add logic for heartbeats, failure service etc.

    fn run(&self) {
        // Use the node's full socket address (e.g., "127.0.0.1:8080") as the bind address.
        let listener = create_listener(&self.node.address).expect("Failed to create TCP listener");
        
        // let client_listener: TcpSocket;

        // if self.node.role == PaxosRole::Proposer {
        let ip = self.node.address.split(':').next().unwrap();
        let new_port = format!("{}{}{}{}", self.node.node_id, self.node.node_id, self.node.node_id, self.node.node_id);
        let new_address = format!("{}:{}", ip, new_port);
        let client_listener = create_listener(&new_address).expect("Failed to create TCP listener"); 
        // }

        logger::log_info(&format!(
            "[TCP Server] Listening on address: {:?}",
            self.node.address,
        ));
        loop {

           if let Ok((socket, input, output)) = client_listener.accept() {
                match input.read(1024) {
                    Ok(buf) if !buf.is_empty() => {
                        let net_msg = tcp_serializer::deserialize(&buf);
                        logger::log_info(&format!("[TCP CLient Reqiest Server] Received message: {:?}", net_msg));

                        
                        match net_msg.payload.clone() {
                            MessagePayload::ClientRequest(val) => {
                                logger::log_info(&format!("[TCP CLient Reqiest Server] Received client request: {:?}", val));
                                let request_id = val.client_id * 1000 + val.client_seq;
                                match self.paxos_agent {
                                    Agent::Proposer(ref agent) => {
                                        agent.submit_client_request(&val);
                                        // self.output_streams.borrow_mut().insert(request_id, output);


                                        if let Some(connection) = self.connections.borrow_mut().remove(&request_id) {
                                            connection.shutdown();
                                        }

                                        let connection = Connection::new(socket, input, output);

                                        self.connections.borrow_mut().insert(request_id, connection);

                                        logger::log_info(&format!("[TCP CLient Reqiest Server] Added output stream for request_id: {}", request_id));
                                    }
                                    _ => {
                                        logger::log_warn(
                                            "[TCP Server] Unknown node role; message not handled.",
                                        );
                                        continue;
                                    }
                                }
                            }

                            _ => {
                                logger::log_warn(&format!("[TCP CLient Reqiest Server] Unknown message type: {:?}", net_msg.payload));
                            }
                        };

                        // let response = tcp_serializer::serialize(&net_msg.clone());

                        // if let Err(e) = output.write(&response.as_slice()) {
                        //     logger::log_warn(&format!("[TCP Server] Write error: {:?}", e));
                        // } else {
                        //     if let Err(e) = output.flush() {
                        //         logger::log_warn(&format!("[TCP Server] Flush error: {:?}", e));
                        //     } else {
                        //         logger::log_info("[TCP Server] Response sent back to client.");
                        //     }
                        // }                        
                    }
                    Ok(_) => { /* No data received */ }
                    Err(e) => {
                        logger::log_warn(&format!("[TCP Server] Read error: {:?}", e));
                    }
                }
            }

            // Accept a connection, if available (non-blocking).
            match listener.accept() {
                Ok((_, input, output)) => {
                    match input.read(1024) {
                        Ok(buf) if !buf.is_empty() => {
                            let net_msg = tcp_serializer::deserialize(&buf);
                            logger::log_info(&format!("[TCP Algorithm Request Server] Received message: {:?}", net_msg));
    
    
                            let response_msg = match self.paxos_agent {
                                Agent::Proposer(ref agent) => agent.handle_message(&net_msg),
                                Agent::Acceptor(ref agent) => agent.handle_message(&net_msg),
                                Agent::Learner(ref agent) => agent.handle_message(&net_msg),
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
                Err(e) if e == TcpErrorCode::WouldBlock => {
                    // No connection available, continue the loop.
                    // do nothing
                }
                Err(e) => {
                    logger::log_warn(&format!("[TCP Server] Accept error: {:?}", e));
                }
                
            }
            match self.paxos_agent {
                Agent::Proposer(ref agent) => {
                    let response = agent.run_paxos_loop();
                    if let Some(responses) = response {
                        for resp in responses {

                            // TODO : THIS COULD BE MORE CLEAN

                            let client_id: u64 = resp.client_id.clone().parse().unwrap_or(0);
                            let request_id = client_id * 1000 + resp.client_seq;

                            let val = Value {
                                is_noop: false,
                                command: resp.command_result,
                                client_id,
                                client_seq: resp.client_seq,
                            };

                            let message = NetworkMessage {
                                sender: self.node.clone(),
                                payload: MessagePayload::ClientRequest(val),
                            };

                            if let Some(connection) = self.connections.borrow_mut().remove(&request_id) {
                                let response_bytes = tcp_serializer::serialize(&message);
                                if let Some(ref output) = connection.output {
                                    if let Err(e) = output.write(response_bytes.as_slice()) {
                                        connection.shutdown();
                                        logger::log_warn(&format!("[TCP Server] Write error: {:?}", e));
                                    } else {
                                        connection.shutdown();
                                        logger::log_info("[TCP Server] Response sent back to client.");
                                    }
                                } else {
                                    connection.shutdown();
                                    logger::log_warn(&format!("[TCP Server] No output stream found for request_id: {}", request_id));
                                }
                            }
                        }
                    }
                },
                Agent::Learner(ref agent) => agent.run_paxos_loop(),
                _ => {
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
