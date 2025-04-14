use std::net::SocketAddr;
use std::thread;
use std::time::Duration;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "paxos-tcp-client-world",
        additional_derives: [PartialEq, Clone],
    });
}

// Export our network client component.
bindings::export!(PaxosTcpClient with_types_in bindings);

// Bring in the generated types.
use bindings::exports::paxos::default::paxos_tcp_client::Guest;
// use bindings::paxos::default::logger;
use bindings::paxos::default::network_types::{MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{Node, PaxosRole};
// use bindings::paxos::default::paxos_types::Node;
use bindings::paxos::default::tcp_serializer;

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
pub struct PaxosTcpClient;


impl PaxosTcpClient {
    fn read_with_timeout(input: &mut InputStream, timeout: Duration) -> Result<Vec<u8>, &'static str> {
        // Start the timer.
        let start = std::time::Instant::now();
        // let mut buf = vec![0u8; 1024];
    
        loop {
            match input.read(1024) {
                Ok(buf) if !buf.is_empty() => {
                    return Ok(buf);
                },
                Ok(_) => { /* No data available yet */ },
                Err(_) => return Err("Encountered an error while reading"),
            }
            
            // Check if we have exceeded the timeout.
            if start.elapsed() >= timeout {
                return Err("Read timed out");
            }
    
            // Sleep briefly to avoid a busy loop.
            thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Implement the network interface functions.
impl Guest for PaxosTcpClient {
    /// send-hello simply returns a static greeting.
    fn perform_request(message: NetworkMessage) -> Option<NetworkMessage> {
        let leader_address = "127.0.0.1:7777";
        let (socket, mut input, output) = create_client_socket(leader_address).unwrap();
        let msg_bytes = tcp_serializer::serialize(&message);


        // socket.subscribe().block();
        
        if output.write(&msg_bytes).is_ok() {
            let timeout = Duration::from_secs(1);
            match PaxosTcpClient::read_with_timeout(&mut input, timeout) {
                Ok(buf) => {
                    // Use the shared serializer to convert the received bytes back to a NetworkMessage.
                    let resp_msg = tcp_serializer::deserialize(&buf);
                    return Some(resp_msg);
                },
                Err(e) => {
                    // logger::log_warn(&format!("[TCP Client] Error: {}", e));
                }
            }
        } 
        
        // logger::log_warn("[TCP Client] It failed.");
        // message
        None
    }
}
