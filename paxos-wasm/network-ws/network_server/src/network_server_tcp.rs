use std::cell::RefCell;
use std::net::SocketAddr;
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::{ErrorCode as TcpErrorCode, TcpSocket};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

pub mod bindings {
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

pub struct MyNetworkServerTcp;
pub struct MyNetworkServerTcpResource {
    listener: RefCell<Option<TcpSocket>>,
}

impl Guest for MyNetworkServerTcp {
    type NetworkServerResource = MyNetworkServerTcpResource;
}

impl GuestNetworkServerResource for MyNetworkServerTcpResource {
    fn new() -> Self {
        Self {
            listener: RefCell::new(None),
        }
    }

    fn setup_listener(&self, bind_addr: String) {
        let net = instance_network();
        let socket = create_tcp_socket(IpAddressFamily::Ipv4).expect("failed to create TCP socket");
        let poll = socket.subscribe();

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

        socket.start_bind(&net, local).unwrap();
        poll.block();
        socket.finish_bind().unwrap();
        socket.start_listen().unwrap();
        poll.block();
        socket.finish_listen().unwrap();

        *self.listener.borrow_mut() = Some(socket);
    }

    fn get_messages(&self) -> Vec<NetworkMessage> {
        let mut out = Vec::new();
        let binding = self.listener.borrow();
        let listener = binding.as_ref().expect("listener not initialized");

        // 1) Accept *all* pending connections (non-blocking)
        loop {
            match listener.accept() {
                Ok((_sock, in_stream, _out_stream)) => {
                    // 2) Do one non-blocking read
                    match in_stream.blocking_read(4096) {
                        Ok(buf) if !buf.is_empty() => {
                            // Deserialize and collect
                            let msg = serializer::deserialize(&buf);
                            out.push(msg);
                        }
                        Ok(_empty) => {
                            // peer closed cleanly without data → ignore
                        }
                        Err(e) => {
                            logger::log_error(&format!("[Network Server] read error: {:?}", e));
                        }
                    }
                    // in_stream/out_stream dropped here
                }
                Err(TcpErrorCode::WouldBlock) => {
                    // no more connections right now
                    break;
                }
                Err(e) => {
                    logger::log_error(&format!("[Network Server] accept error: {:?}", e));
                    break;
                }
            }
        }

        out
    }

    fn get_message(&self) -> Option<NetworkMessage> {
        self.get_messages().into_iter().next()
    }
}
