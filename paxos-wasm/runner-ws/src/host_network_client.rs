use std::collections::HashMap;
use std::io::Write;
use std::net::TcpStream;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::bindings::paxos::default::network_types::NetworkMessage;
use crate::bindings::paxos::default::paxos_types::Node;
use crate::host_serializer::{HostSerializer, MyHostSerializer};

pub struct NativeTcpClient {
    tx: Sender<(String, Vec<u8>)>,
}

impl NativeTcpClient {
    pub fn new() -> Self {
        println!("[Client] Starting TCP client thread");
        let (tx, rx) = mpsc::channel::<(String, Vec<u8>)>();
        let conns = Arc::new(Mutex::new(HashMap::new()));

        let conns_bg = Arc::clone(&conns);

        thread::spawn(move || {
            while let Ok((addr, msg)) = rx.recv() {
                let mut conns = conns_bg.lock().unwrap();

                let stream = conns.entry(addr.clone()).or_insert_with(|| {
                    match TcpStream::connect(&addr) {
                        Ok(s) => {
                            s.set_nodelay(true).ok();
                            s
                        }
                        Err(e) => {
                            eprintln!("[Client] Failed to connect to {}: {}", addr, e);
                            return TcpStream::connect("0.0.0.0:0").unwrap(); // dummy stream; real impl should skip
                        }
                    }
                });

                if let Err(e) = stream.write_all(&msg) {
                    eprintln!("[Client] Failed to write to {}: {}", addr, e);
                    conns.remove(&addr);
                }
            }
        });

        Self { tx }
    }

    pub fn send_message_forget(&self, nodes: Vec<Node>, message: NetworkMessage) {
        let msg_bytes = MyHostSerializer::serialize(message);

        for node in nodes {
            let addr = node.address;
            let _ = self.tx.send((addr, msg_bytes.clone()));
        }
    }

    pub fn send_message(&self, _nodes: Vec<Node>, _message: NetworkMessage) -> Vec<NetworkMessage> {
        // let msg_bytes = MyHostSerializer::serialize(message);
        // let mut replies = vec![];

        // for node in nodes {
        //     let addr = node.address;
        //     let _ = self.tx.send((addr, msg_bytes.clone()));
        //     // Here we would normally wait for a reply, but this is a fire-and-forget example.
        // }

        // replies
        todo!()
    }
}
