use std::cmp::min;
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::bindings::paxos::default::network_types::NetworkMessage;
use crate::host_serializer::{HostSerializer, MyHostSerializer};

pub struct NativeTcpServer {
    messages: Arc<Mutex<VecDeque<NetworkMessage>>>,
}

impl NativeTcpServer {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn setup_listener(&mut self, bind_addr: &str) {
        let listener = TcpListener::bind(bind_addr).expect("failed to bind listener");
        listener
            .set_nonblocking(true)
            .expect("failed to set non-blocking");
        println!("[Server] Listening on {}", bind_addr);

        let messages = Arc::clone(&self.messages);

        thread::spawn(move || {
            let mut next_id = 1u64;
            let mut conns: HashMap<u64, TcpStream> = HashMap::new();
            let mut buffers: HashMap<u64, Vec<u8>> = HashMap::new();

            loop {
                // Accept new connections
                match listener.accept() {
                    Ok((stream, _addr)) => {
                        stream.set_nonblocking(true).unwrap();
                        let id = next_id;
                        next_id += 1;
                        conns.insert(id, stream);
                        buffers.insert(id, Vec::new());
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {} // No new connection
                    Err(e) => {
                        eprintln!("[Server] Accept error: {}", e);
                    }
                }

                let mut to_remove = vec![];

                // Read from all connections
                for (&id, stream) in conns.iter_mut() {
                    let mut buf = [0u8; 4096];
                    match stream.read(&mut buf) {
                        Ok(0) => {
                            // Connection closed
                            to_remove.push(id);
                        }
                        Ok(n) => {
                            let b = buffers.get_mut(&id).unwrap();
                            b.extend_from_slice(&buf[..n]);

                            let mut offset = 0;
                            while b.len() >= offset + 4 {
                                let len =
                                    u32::from_be_bytes(b[offset..offset + 4].try_into().unwrap())
                                        as usize;

                                if b.len() < offset + 4 + len {
                                    break;
                                }

                                let mut frame = (len as u32).to_be_bytes().to_vec();
                                frame.extend_from_slice(&b[offset + 4..offset + 4 + len]);
                                offset += 4 + len;

                                if let Ok(msg) = MyHostSerializer::deserialize(frame) {
                                    messages.lock().unwrap().push_back(msg);
                                } else {
                                    eprintln!("[Server] Failed to deserialize message from {}", id);
                                }
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
                    conns.remove(&id);
                    buffers.remove(&id);
                }

                thread::sleep(Duration::from_millis(1)); // Prevent busy spinning
            }
        });
    }

    pub fn get_messages(&self, max: u64) -> Vec<NetworkMessage> {
        let mut queue = self.messages.lock().unwrap();
        let take = min(max as usize, queue.len());
        queue.drain(..take).collect()
    }

    pub fn get_message(&self) -> Option<NetworkMessage> {
        self.messages.lock().unwrap().pop_front()
    }
}
