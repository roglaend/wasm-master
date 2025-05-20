use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;

use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::{ErrorCode as TcpErrorCode, InputStream, OutputStream, TcpSocket};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "network-server-world",
        additional_derives: [PartialEq, Clone],
    });
}
bindings::export!(MyNetworkServerTcp with_types_in bindings);

use bindings::exports::paxos::default::network_server::{Guest, GuestNetworkServerResource};
use bindings::paxos::default::network_types::NetworkMessage;
use bindings::paxos::default::{logger, serializer};

struct MyNetworkServerTcp;
struct MyNetworkServerTcpResource {
    /// the listening socket
    listener: RefCell<Option<TcpSocket>>,
    /// simple increasing connection ID
    next_chan: RefCell<u64>,
    /// live connections: chan_id → (socket, reader, writer)
    conns: RefCell<HashMap<u64, (TcpSocket, InputStream, OutputStream)>>,
    /// buffer bytes per chan until we have a full frame
    buffers: RefCell<HashMap<u64, Vec<u8>>>,
}

impl Guest for MyNetworkServerTcp {
    type NetworkServerResource = MyNetworkServerTcpResource;
}

impl GuestNetworkServerResource for MyNetworkServerTcpResource {
    fn new() -> Self {
        Self {
            listener: RefCell::new(None),
            next_chan: RefCell::new(1),
            conns: RefCell::new(HashMap::new()),
            buffers: RefCell::new(HashMap::new()),
        }
    }

    fn setup_listener(&self, bind_addr: String) {
        let net = instance_network();
        let sock = create_tcp_socket(IpAddressFamily::Ipv4).expect("failed to create TCP socket");
        let poll = sock.subscribe();

        // parse and bind
        let std_addr: SocketAddr = bind_addr.parse().expect("invalid bind address");
        let local = if let SocketAddr::V4(v4) = std_addr {
            let o = v4.ip().octets();
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                address: (o[0], o[1], o[2], o[3]),
                port: v4.port(),
            })
        } else {
            panic!("only IPv4 supported");
        };
        sock.start_bind(&net, local).unwrap();
        poll.block();
        sock.finish_bind().unwrap();

        // start listening
        sock.start_listen().unwrap();
        poll.block();
        sock.finish_listen().unwrap();

        *self.listener.borrow_mut() = Some(sock);
    }

    fn get_messages(&self, max: u64) -> Vec<NetworkMessage> {
        let max = max as usize;
        let mut out = Vec::new();
        let mut to_drop = Vec::new();

        // 1) Accept any new connections
        if let Some(listener) = self.listener.borrow().as_ref() {
            loop {
                match listener.accept() {
                    Ok((sock, reader, writer)) => {
                        let chan = *self.next_chan.borrow();
                        *self.next_chan.borrow_mut() += 1;
                        self.conns.borrow_mut().insert(chan, (sock, reader, writer));
                        self.buffers.borrow_mut().insert(chan, Vec::new());
                    }
                    Err(TcpErrorCode::WouldBlock) => break,
                    Err(e) => {
                        logger::log_error(&format!("[Network Server] accept error: {:?}", e));
                        break;
                    }
                }
            }
        }

        // 2) Read from all live connections, frame-decode, deserialize
        for (&chan, (_sock, reader, _writer)) in self.conns.borrow().iter() {
            match reader.read(65536) {
                Ok(chunk) if !chunk.is_empty() => {
                    let mut buffers = self.buffers.borrow_mut();
                    let buf = buffers.get_mut(&chan).unwrap();
                    buf.extend_from_slice(&chunk);

                    // pull out as many complete length-prefixed frames as possible
                    let mut offset = 0;
                    while buf.len() - offset >= 4 {
                        let len = u32::from_be_bytes(buf[offset..offset + 4].try_into().unwrap())
                            as usize;
                        if buf.len() < offset + 4 + len {
                            break;
                        }

                        // reattach header so serializer still works
                        let mut frame = (len as u32).to_be_bytes().to_vec();
                        frame.extend_from_slice(&buf[offset + 4..offset + 4 + len]);
                        offset += 4 + len;

                        // deserialize and collect
                        let msg: NetworkMessage = serializer::deserialize(&frame);
                        out.push(msg);
                    }

                    if offset > 0 {
                        buf.drain(0..offset);
                    }

                    // break the `for` if we’ve reached max
                    if out.len() >= max {
                        break;
                    }
                }
                Ok(_) => {
                    // Nothing to read. Can't be used to detect FIN/closed connection. Do nothing.
                }
                Err(e) => {
                    logger::log_error(&format!(
                        "[Network Server] read error on chan {}: {:?}, dropping",
                        chan, e
                    ));
                    to_drop.push(chan);
                }
            }
        }

        // 3) Clean up any broken connections
        if !to_drop.is_empty() {
            let mut conns = self.conns.borrow_mut();
            let mut bufs = self.buffers.borrow_mut();
            for chan in to_drop {
                if let Some((sock, reader, writer)) = conns.remove(&chan) {
                    drop(reader);
                    drop(writer);
                    drop(sock);
                }
                bufs.remove(&chan);
            }
        }

        out
    }

    fn get_message(&self) -> Option<NetworkMessage> {
        self.get_messages(1).into_iter().next()
    }
}
