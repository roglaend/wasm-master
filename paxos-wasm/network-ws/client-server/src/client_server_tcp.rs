use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;

use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::{ErrorCode as TcpErrorCode, InputStream, OutputStream, TcpSocket};

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit/",
        world: "client-server-world",
        additional_derives: [PartialEq, Clone],
    });
}
bindings::export!(MyClientServerTcp with_types_in bindings);

use bindings::exports::paxos::default::client_server::{Guest, GuestClientServerResource};
use bindings::paxos::default::network_types::{MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{ClientResponse, Node, Value};
use bindings::paxos::default::{logger, serializer};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

pub struct MyClientServerTcp;
pub struct MyClientServerTcpResource {
    listener: RefCell<Option<TcpSocket>>,
    next_chan: RefCell<u64>,

    /// All live connections, keyed by our own channel ID.
    conns: RefCell<HashMap<u64, (TcpSocket, InputStream, OutputStream)>>,

    /// Accumulate bytes per‐channel until a full frame is available
    buffers: RefCell<HashMap<u64, Vec<u8>>>,

    /// Temporary map from (client_id, client_seq) → which chan_id carried that request.
    req2chan: RefCell<HashMap<(String, u64), u64>>,
}

impl Guest for MyClientServerTcp {
    type ClientServerResource = MyClientServerTcpResource;
}

impl GuestClientServerResource for MyClientServerTcpResource {
    fn new() -> Self {
        Self {
            listener: RefCell::new(None),
            next_chan: RefCell::new(1),
            conns: RefCell::new(HashMap::new()),
            buffers: RefCell::new(HashMap::new()),
            req2chan: RefCell::new(HashMap::new()),
        }
    }

    fn setup_listener(&self, bind_addr: String) {
        let net = instance_network();
        let sock = create_tcp_socket(IpAddressFamily::Ipv4).expect("failed to create TCP socket");
        let poll = sock.subscribe();

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
        sock.start_listen().unwrap();
        poll.block();
        sock.finish_listen().unwrap();

        *self.listener.borrow_mut() = Some(sock);
    }

    fn get_requests(&self, max: u64) -> Vec<Value> {
        let max = max as usize;

        // Stage 1: Accept new connections + read frames
        let (out, mut to_drop) = {
            let mut conns = self.conns.borrow_mut();
            let mut bufs = self.buffers.borrow_mut();
            let mut out = Vec::new();
            let mut to_drop = Vec::new();

            // 1) Accept all new connections
            if let Some(listener) = self.listener.borrow().as_ref() {
                loop {
                    match listener.accept() {
                        Ok((sock, in_s, out_s)) => {
                            let chan = *self.next_chan.borrow();
                            *self.next_chan.borrow_mut() += 1;
                            conns.insert(chan, (sock, in_s, out_s));
                            bufs.insert(chan, Vec::new());
                        }
                        Err(TcpErrorCode::WouldBlock) => break,
                        Err(e) => {
                            logger::log_error(&format!(
                                "[Client Server] connection accept error: {:?}",
                                e
                            ));
                            break;
                        }
                    }
                }
            }

            // 2) Read / accumulate / frame‐decode
            for (&chan, (_sock, in_s, _out_s)) in conns.iter_mut() {
                match in_s.read(65536) {
                    Ok(chunk) if !chunk.is_empty() => {
                        let buf = bufs.get_mut(&chan).unwrap();
                        buf.extend_from_slice(&chunk);

                        // extract as many complete frames as possible
                        let mut offset = 0;
                        while buf.len() - offset >= 4 {
                            let len =
                                u32::from_be_bytes(buf[offset..offset + 4].try_into().unwrap())
                                    as usize;
                            if buf.len() - offset < 4 + len {
                                break;
                            }
                            let frame = buf[offset + 4..offset + 4 + len].to_vec();
                            offset += 4 + len;

                            let mut with_header = (len as u32).to_be_bytes().to_vec();
                            with_header.extend_from_slice(&frame);
                            let msg: NetworkMessage = serializer::deserialize(&with_header);

                            if let MessagePayload::ClientRequest(v) = msg.payload {
                                self.req2chan
                                    .borrow_mut()
                                    .insert((v.client_id.clone(), v.client_seq), chan);
                                out.push(v);
                            }
                        }

                        if offset > 0 {
                            buf.drain(0..offset);
                        }

                        // if we've hit max, break out of the *for* loop
                        if out.len() >= max {
                            break;
                        }
                    }
                    Ok(_) => {
                        // Nothing to read. Can't be used to detect FIN/closed connection. Do nothing.
                    }
                    Err(_e) => {
                        // TODO: Annoying log when 500 clients are finished
                        // logger::log_error(&format!(
                        //     "[Client Server] read error on chan {}: {:?}, dropping",
                        //     chan, e
                        // ));
                        to_drop.push(chan);
                    }
                }
            }

            (out, to_drop)
        };
        // Stage 2: cleanup dropped channels
        {
            let mut conns = self.conns.borrow_mut();
            let mut req2chan = self.req2chan.borrow_mut();

            for chan in to_drop.drain(..) {
                if let Some((sock, in_s, out_s)) = conns.remove(&chan) {
                    // 1) drop the streams first, so their WASI children go away
                    drop(in_s);
                    drop(out_s);
                    // 2) then drop the socket itself
                    drop(sock);
                }
                // also forget any mapping for this channel
                req2chan.retain(|_, &mut c| c != chan);
            }
        }

        out
    }

    fn get_request(&self) -> Option<Value> {
        self.get_requests(1).into_iter().next()
    }

    fn send_responses(&self, sender: Node, replies: Vec<ClientResponse>) {
        let mut req2chan = self.req2chan.borrow_mut();
        let conns = self.conns.borrow();
        let mut to_drop = Vec::new();

        for resp in replies {
            let key = (resp.client_id.clone(), resp.client_seq);
            if let Some(&chan) = req2chan.get(&key) {
                if let Some((_, _, out_s)) = conns.get(&chan) {
                    let nm = NetworkMessage {
                        sender: sender.clone(),
                        payload: MessagePayload::ClientResponse(resp),
                    };
                    let bytes = serializer::serialize(&nm);
                    if out_s.blocking_write_and_flush(&bytes).is_err() {
                        logger::log_error(&format!(
                            "[Client Server] write error on chan {} → dropping",
                            chan
                        ));
                        to_drop.push(chan);
                    }
                }
                req2chan.remove(&key);
            }
        }

        // Tear down any broken channels exactly as in get_requests
        if !to_drop.is_empty() {
            let mut conns = self.conns.borrow_mut();
            let mut req2chan = self.req2chan.borrow_mut();

            for chan in to_drop.drain(..) {
                if let Some((sock, in_s, out_s)) = conns.remove(&chan) {
                    drop(in_s);
                    drop(out_s);
                    drop(sock);
                }
                req2chan.retain(|_, &mut c| c != chan);
            }
        }
    }
}
