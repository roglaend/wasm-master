#![allow(dead_code)]
#![allow(warnings)]

use std::collections::HashMap;
use std::io::Read;
use std::net::{TcpListener, TcpStream};

use crate::serializer::{MySerializer, Serializer};

use crate::bindings::paxos::default::network_types::NetworkMessage;

pub struct NativeTcpServer {
    listener: Option<TcpListener>,
    next_id: u64,
    conns: HashMap<u64, TcpStream>,
    buffers: HashMap<u64, Vec<u8>>,
    serializer: MySerializer,
}

impl NativeTcpServer {
    pub fn new() -> Self {
        Self {
            listener: None,
            next_id: 1,
            conns: HashMap::new(),
            buffers: HashMap::new(),
            serializer: MySerializer,
        }
    }

    pub fn setup_listener(&mut self, bind_addr: &str) {
        let listener = TcpListener::bind(bind_addr).expect("failed to bind listener");
        listener
            .set_nonblocking(true)
            .expect("failed to set non-blocking");
        self.listener = Some(listener);
        println!("[Server] Listening on {}", bind_addr);
    }

    pub fn get_messages(&mut self) -> Vec<NetworkMessage> {
        let mut messages = vec![];
        let mut to_remove = vec![];

        // Accept new connections
        if let Some(listener) = self.listener.as_ref() {
            loop {
                match listener.accept() {
                    Ok((stream, _addr)) => {
                        stream.set_nonblocking(true).unwrap();
                        let id = self.next_id;
                        self.next_id += 1;
                        self.conns.insert(id, stream);
                        self.buffers.insert(id, vec![]);
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        eprintln!("[Server] Accept error: {}", e);
                        break;
                    }
                }
            }
        }

        // Read from connections

        for (&id, stream) in self.conns.iter_mut() {
            let mut buf = [0u8; 4096];
            match stream.read(&mut buf) {
                Ok(0) => to_remove.push(id),
                Ok(n) => {
                    let b = self.buffers.get_mut(&id).unwrap();
                    b.extend_from_slice(&buf[..n]);

                    let mut offset = 0;
                    while b.len() >= offset + 4 {
                        let len =
                            u32::from_be_bytes(b[offset..offset + 4].try_into().unwrap()) as usize;
                        if b.len() < offset + 4 + len {
                            break;
                        }
                        let mut frame = (len as u32).to_be_bytes().to_vec();
                        frame.extend_from_slice(&b[offset + 4..offset + 4 + len]);
                        offset += 4 + len;

                        let msg = self.serializer.deserialize(frame).unwrap();
                        messages.push(msg);
                    }

                    if offset > 0 {
                        b.drain(0..offset);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    eprintln!("[Server] Read error on {}: {}", id, e);
                    to_remove.push(id);
                }
            }
        }

        for id in to_remove {
            self.conns.remove(&id);
            self.buffers.remove(&id);
        }

        messages
    }

    pub fn get_message(&mut self) -> Option<NetworkMessage> {
        self.get_messages().into_iter().next()
    }
}
