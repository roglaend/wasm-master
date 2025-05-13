use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;

use crate::bindings::paxos::default::network_types::NetworkMessage;
use crate::bindings::paxos::default::paxos_types::Node;

use crate::serializer::{MySerializer, Serializer};

pub struct NativeTcpClient {
    conns: HashMap<String, TcpStream>,
    bufs: HashMap<String, Vec<u8>>,
    serializer: MySerializer,
}

impl NativeTcpClient {
    pub fn new() -> Self {
        Self {
            conns: HashMap::new(),
            bufs: HashMap::new(),
            serializer: MySerializer,
        }
    }

    fn ensure_conn(&mut self, addr: &str) -> std::io::Result<()> {
        if !self.conns.contains_key(addr) {
            let mut stream = TcpStream::connect(addr)?;
            stream.set_nodelay(true)?;
            stream.set_nonblocking(true)?;
            self.conns.insert(addr.to_string(), stream);
            self.bufs.insert(addr.to_string(), vec![]);
            println!("[Client] Connected to {}", addr);
        }
        Ok(())
    }

    pub fn send_message_forget(&mut self, nodes: Vec<Node>, message: NetworkMessage) {
        let msg_bytes = self.serializer.serialize(message.clone());

        for node in &nodes {
            let addr = &node.address;
            if self.ensure_conn(addr).is_err() {
                continue;
            }

            if let Some(stream) = self.conns.get_mut(addr) {
                if let Err(e) = stream.write_all(&msg_bytes) {
                    eprintln!("[Client] Write to {} failed (forget): {}", addr, e);
                }
            }
        }
    }

    pub fn send_message(&mut self, nodes: Vec<Node>, message: &[u8]) -> Vec<NetworkMessage> {
        let mut replies = vec![];
        let mut frame = (message.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(message);

        for node in &nodes {
            let addr = &node.address;
            if self.ensure_conn(addr).is_err() {
                continue;
            }

            if let Some(stream) = self.conns.get_mut(addr) {
                if let Err(e) = stream.write_all(&frame) {
                    eprintln!("[Client] Write to {} failed: {}", addr, e);
                }
            }

            if let Some(stream) = self.conns.get_mut(addr) {
                let buf = self.bufs.get_mut(addr).unwrap();
                let mut chunk = [0u8; 4096];
                match stream.read(&mut chunk) {
                    Ok(n) if n > 0 => {
                        buf.extend_from_slice(&chunk[..n]);
                        let mut offset = 0;
                        while buf.len() >= offset + 4 {
                            let len =
                                u32::from_be_bytes(buf[offset..offset + 4].try_into().unwrap())
                                    as usize;
                            if buf.len() < offset + 4 + len {
                                break;
                            }
                            let mut frame = (len as u32).to_be_bytes().to_vec();
                            frame.extend_from_slice(&buf[offset + 4..offset + 4 + len]);
                            offset += 4 + len;

                            let msg: NetworkMessage = self.serializer.deserialize(frame).unwrap();
                            replies.push(msg);
                        }
                        if offset > 0 {
                            buf.drain(0..offset);
                        }
                    }
                    _ => {}
                }
            }
        }

        replies
    }
}
