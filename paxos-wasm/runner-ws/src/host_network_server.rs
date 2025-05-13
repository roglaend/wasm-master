// use crate::bindings::paxos::default::network_types::NetworkMessage;
// use crate::serializer::{MySerializer, Serializer};
// use std::sync::{Arc, Mutex};
// use tokio::io::AsyncReadExt;
// use tokio::net::TcpListener;
// use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

// pub struct NativeTcpServer {
//     sender: UnboundedSender<NetworkMessage>,
//     receiver: Arc<Mutex<UnboundedReceiver<NetworkMessage>>>,
//     serializer: MySerializer,
// }

// impl NativeTcpServer {
//     pub fn new() -> Self {
//         let (sender, receiver) = unbounded_channel();
//         Self {
//             sender,
//             receiver: Arc::new(Mutex::new(receiver)),
//             serializer: MySerializer,
//         }
//     }

//     /// Launches a dedicated thread with a Tokio runtime for the listener.
//     pub fn setup_listener(&mut self, bind_addr: &str) {
//         let addr = bind_addr.to_string();
//         let serializer = self.serializer.clone();
//         let sender = self.sender.clone();

//         std::thread::spawn(move || {
//             let rt = tokio::runtime::Builder::new_multi_thread()
//                 .worker_threads(2)
//                 .enable_all()
//                 .build()
//                 .expect("Failed to build Tokio runtime");

//             rt.block_on(async move {
//                 let listener = TcpListener::bind(&addr)
//                     .await
//                     .expect("Failed to bind listener");
//                 println!("[Server] Listening on {}", addr);

//                 loop {
//                     match listener.accept().await {
//                         Ok((mut stream, _)) => {
//                             let serializer = serializer.clone();
//                             let sender = sender.clone();

//                             tokio::spawn(async move {
//                                 let mut buf = vec![0u8; 4096];
//                                 let mut buffer = Vec::new();

//                                 loop {
//                                     match stream.read(&mut buf).await {
//                                         Ok(0) => {
//                                             println!("[Server] Connection closed");
//                                             break;
//                                         }
//                                         Ok(n) => {
//                                             buffer.extend_from_slice(&buf[..n]);
//                                             let mut offset = 0;

//                                             while buffer.len() >= offset + 4 {
//                                                 let len = u32::from_be_bytes(
//                                                     buffer[offset..offset + 4].try_into().unwrap(),
//                                                 )
//                                                     as usize;

//                                                 if buffer.len() < offset + 4 + len {
//                                                     break;
//                                                 }

//                                                 let mut frame = (len as u32).to_be_bytes().to_vec();
//                                                 frame.extend_from_slice(
//                                                     &buffer[offset + 4..offset + 4 + len],
//                                                 );
//                                                 offset += 4 + len;

//                                                 match serializer.deserialize(frame) {
//                                                     Ok(msg) => {
//                                                         let _ = sender.send(msg);
//                                                     }
//                                                     Err(e) => {
//                                                         eprintln!(
//                                                             "[Server] Deserialize error: {:?}",
//                                                             e
//                                                         );
//                                                     }
//                                                 }
//                                             }

//                                             if offset > 0 {
//                                                 buffer.drain(0..offset);
//                                             }
//                                         }
//                                         Err(e) => {
//                                             eprintln!("[Server] Read error: {}", e);
//                                             break;
//                                         }
//                                     }
//                                 }
//                             });
//                         }
//                         Err(e) => {
//                             eprintln!("[Server] Accept error: {}", e);
//                         }
//                     }
//                 }
//             });
//         });
//     }

//     /// Drains all received messages from the queue.
//     pub fn get_messages(&self) -> Vec<NetworkMessage> {
//         let mut rx = self.receiver.lock().unwrap();
//         let mut out = Vec::new();

//         while let Ok(msg) = rx.try_recv() {
//             out.push(msg);
//         }

//         out
//     }

//     pub fn get_message(&self) -> Option<NetworkMessage> {
//         self.get_messages().into_iter().next()
//     }
// }
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::bindings::paxos::default::network_types::NetworkMessage;
use crate::serializer::{MySerializer, Serializer};

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
            let serializer = MySerializer;

            loop {
                // Accept new connections
                match listener.accept() {
                    Ok((mut stream, _addr)) => {
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

                                if let Ok(msg) = serializer.deserialize(frame) {
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

    pub fn get_messages(&self) -> Vec<NetworkMessage> {
        let mut queue = self.messages.lock().unwrap();
        queue.drain(..).collect()
    }

    pub fn get_message(&self) -> Option<NetworkMessage> {
        self.messages.lock().unwrap().pop_front()
    }
}
