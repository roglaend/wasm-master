use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::thread;
use std::time::Duration;

use wasi::io::streams::{InputStream, OutputStream};
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::{ErrorCode as TcpErrorCode, ShutdownType, TcpSocket};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "paxos-client-world",
        additional_derives: [PartialEq, Clone],
    });
}
bindings::export!(PaxosClient with_types_in bindings);

use bindings::exports::paxos::default::paxos_client::{Guest, GuestPaxosClientResource};
use bindings::paxos::default::network_types::{MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{ClientResponse, Node, PaxosRole, Value};
use bindings::paxos::default::serializer;

/// One‐off helper for the `perform_request` free function.
fn create_client_socket(
    socket_addr_str: &str,
) -> Result<(TcpSocket, InputStream, OutputStream), TcpErrorCode> {
    let network = instance_network();
    let sock = create_tcp_socket(IpAddressFamily::Ipv4)?;
    let std_addr: SocketAddr = socket_addr_str.parse().expect("invalid address");
    let remote = match std_addr {
        SocketAddr::V4(v4) => {
            let oct = v4.ip().octets();
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                address: (oct[0], oct[1], oct[2], oct[3]),
                port: v4.port(),
            })
        }
        _ => panic!("only IPv4 supported"),
    };
    sock.start_connect(&network, remote)?;
    sock.subscribe().block();
    let (i, o) = sock.finish_connect()?;
    Ok((sock, i, o))
}

/// Blocking‐read with a deadline.
fn read_with_timeout(input: &mut InputStream, timeout: Duration) -> Result<Vec<u8>, ()> {
    let start = std::time::Instant::now();
    loop {
        match input.read(4096) {
            Ok(buf) if !buf.is_empty() => return Ok(buf),
            Ok(_) => {}
            Err(_) => return Err(()),
        }
        if start.elapsed() >= timeout {
            return Err(());
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// A persistent TCP connection plus its streams.
pub struct Connection {
    socket: TcpSocket,
    input: Option<InputStream>,
    output: Option<OutputStream>,
}

impl Connection {
    pub fn new(socket: TcpSocket, input: InputStream, output: OutputStream) -> Self {
        Self {
            socket,
            input: Some(input),
            output: Some(output),
        }
    }

    /// Client is done *sending* requests: shut down the send half and drop our OutputStream.
    pub fn shutdown_send(&mut self) {
        // Tell the OS we will send no more data (FIN to peer)
        let _ = self.socket.shutdown(ShutdownType::Send);
        // Drop our write stream so we can’t send again
        self.output.take();
    }

    /// Client is done *receiving* responses: shut down the receive half and drop our InputStream.
    pub fn shutdown_receive(&mut self) {
        // Tell the OS we are not reading any more (discard inbound)
        let _ = self.socket.shutdown(ShutdownType::Receive);
        self.input.take();
    }

    /// Fully close both halves and drop the socket.
    pub fn shutdown_both(self) {
        // We consume self so drop order is:
        //   - shutdown both directions
        //   - drop streams
        //   - drop socket
        let mut this = self;
        let _ = this.socket.shutdown(ShutdownType::Both);
        let _ = this.input.take();
        let _ = this.output.take();
        // socket is dropped here
    }
}

struct PaxosClient;

impl Guest for PaxosClient {
    type PaxosClientResource = MyPaxosClientResource;

    fn perform_request(leader_address: String, value: Value) -> Option<ClientResponse> {
        let (_sock, mut input, output) = create_client_socket(&leader_address).ok()?;

        let msg = NetworkMessage {
            sender: Node {
                node_id: 0,
                address: "123".into(),
                role: PaxosRole::Client,
            },
            payload: MessagePayload::ClientRequest(value),
        };
        let bytes = serializer::serialize(&msg);
        if output.write(&bytes).is_err() {
            return None;
        }

        if let Ok(buf) = read_with_timeout(&mut input, Duration::from_secs(5)) {
            if let MessagePayload::ClientResponse(resp) = serializer::deserialize(&buf).payload {
                return Some(resp);
            }
        }
        None
    }
}

pub struct MyPaxosClientResource {
    conns: RefCell<HashMap<String, Connection>>,

    /// Buffer of raw bytes per leader addr
    bufs: RefCell<HashMap<String, Vec<u8>>>,
}

impl MyPaxosClientResource {
    fn ensure_conn(&self, addr: &str) -> Result<(), String> {
        let mut m = self.conns.borrow_mut();
        if !m.contains_key(addr) {
            let (sock, i, o) =
                create_client_socket(addr).map_err(|e| format!("connect failed: {:?}", e))?;
            m.insert(addr.to_string(), Connection::new(sock, i, o));
        }
        Ok(())
    }
}

impl GuestPaxosClientResource for MyPaxosClientResource {
    fn new() -> Self {
        Self {
            conns: RefCell::new(HashMap::new()),
            bufs: RefCell::new(HashMap::new()),
        }
    }

    fn send_request(&self, leader_address: String, value: Value) -> bool {
        if self.ensure_conn(&leader_address).is_err() {
            return false;
        }
        let mut map = self.conns.borrow_mut();
        let conn = map.get_mut(&leader_address).unwrap();
        let msg = NetworkMessage {
            sender: Node {
                node_id: 0,
                address: "123".into(),
                role: PaxosRole::Client,
            },
            payload: MessagePayload::ClientRequest(value),
        };
        let bytes = serializer::serialize(&msg);
        conn.output.as_ref().unwrap().write(&bytes).is_ok()
    }

    fn try_receive(&self, leader_address: String) -> Vec<ClientResponse> {
        let mut out = Vec::new();

        // Make sure we have a connection
        if self.ensure_conn(&leader_address).is_err() {
            return out;
        }

        // Borrow connection and buffer
        let mut conns = self.conns.borrow_mut();
        let mut bufs = self.bufs.borrow_mut();
        let conn = conns.get_mut(&leader_address).unwrap();
        let buf = bufs.entry(leader_address.clone()).or_default();
        let input = conn.input.as_mut().unwrap();

        // 1) Read whatever bytes are ready
        if let Ok(chunk) = input.read(4096) {
            if !chunk.is_empty() {
                buf.extend_from_slice(&chunk);
            }
        }

        // 2) Extract all full frames
        let mut offset = 0;
        while buf.len() >= offset + 4 {
            let len = u32::from_be_bytes(buf[offset..offset + 4].try_into().unwrap()) as usize;
            if buf.len() < offset + 4 + len {
                break;
            }

            // Pull out one complete message (excluding the 4-byte header)
            let payload = buf[offset + 4..offset + 4 + len].to_vec();
            offset += 4 + len;

            // Reattach the length header so your existing serializer code still works
            let mut frame = (len as u32).to_be_bytes().to_vec();
            frame.extend_from_slice(&payload);

            // Deserialize and collect any ClientResponse
            let nm = serializer::deserialize(&frame);
            if let MessagePayload::ClientResponse(resp) = nm.payload {
                out.push(resp);
            }
        }

        // 3) Discard any consumed bytes
        if offset > 0 {
            buf.drain(0..offset);
        }

        out
    }

    fn shutdown_send(&self, leader_address: String) {
        if let Some(conn) = self.conns.borrow_mut().get_mut(&leader_address) {
            conn.shutdown_send();
        }
    }

    fn shutdown_receive(&self, leader_address: String) {
        if let Some(conn) = self.conns.borrow_mut().get_mut(&leader_address) {
            conn.shutdown_receive();
        }
    }

    fn close(&self, leader_address: String) {
        if let Some(conn) = self.conns.borrow_mut().remove(&leader_address) {
            conn.shutdown_both();
        }
    }
}
