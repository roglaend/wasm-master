// use std::collections::HashMap;
// use std::sync::{Arc, Mutex};
// use tokio::io::{AsyncReadExt, AsyncWriteExt};
// use tokio::net::TcpStream;
// use tokio::sync::Mutex as AsyncMutex;

// use crate::bindings::paxos::default::network_types::NetworkMessage;
// use crate::bindings::paxos::default::paxos_types::Node;
// use crate::serializer::{MySerializer, Serializer};

// pub struct NativeTcpClient {
//     conns: Arc<AsyncMutex<HashMap<String, TcpStream>>>,
//     bufs: Arc<Mutex<HashMap<String, Vec<u8>>>>,
//     serializer: MySerializer,
// }

// impl NativeTcpClient {
//     pub fn new() -> Self {
//         Self {
//             conns: Arc::new(AsyncMutex::new(HashMap::new())),
//             bufs: Arc::new(Mutex::new(HashMap::new())),
//             serializer: MySerializer,
//         }
//     }

//     /// Ensures a connection exists to a given address.
//     async fn ensure_conn(&self, addr: &str) -> std::io::Result<()> {
//         let mut conns = self.conns.lock().await;
//         if !conns.contains_key(addr) {
//             let stream = TcpStream::connect(addr).await?;
//             conns.insert(addr.to_string(), stream);
//             self.bufs
//                 .lock()
//                 .unwrap()
//                 .insert(addr.to_string(), Vec::new());
//             println!("[Client] Connected to {}", addr);
//         }
//         Ok(())
//     }

//     /// Fire-and-forget message send; this method returns immediately.
//     pub fn send_message_forget(&self, nodes: Vec<Node>, message: NetworkMessage) {
//         let msg_bytes = self.serializer.serialize(message);
//         let conns = Arc::clone(&self.conns);
//         let this = self.clone();

//         tokio::spawn(async move {
//             for node in &nodes {
//                 let addr = &node.address;
//                 if this.ensure_conn(addr).await.is_err() {
//                     continue;
//                 }

//                 let mut conns_guard = conns.lock().await;
//                 if let Some(stream) = conns_guard.get_mut(addr) {
//                     if let Err(e) = stream.write_all(&msg_bytes).await {
//                         eprintln!("[Client] Write to {} failed (forget): {}", addr, e);
//                     }
//                 }
//             }
//         });
//     }

//     /// Synchronous-looking send message method that awaits a reply.
//     pub fn send_message(&self, nodes: Vec<Node>, message: NetworkMessage) -> Vec<NetworkMessage> {
//         let mut replies = vec![];
//         // let mut frame = (message.len() as u32).to_be_bytes().to_vec();
//         // frame.extend_from_slice(message);

//         // let mut conns = self.conns.lock().await;

//         // for node in &nodes {
//         //     let addr = &node.address;

//         //     if self.ensure_conn(addr).await.is_err() {
//         //         continue;
//         //     }

//         //     if let Some(stream) = conns.get_mut(addr) {
//         //         if let Err(e) = stream.write_all(&frame).await {
//         //             eprintln!("[Client] Write to {} failed: {}", addr, e);
//         //             continue;
//         //         }

//         //         let mut buf = [0u8; 4096];
//         //         match stream.read(&mut buf).await {
//         //             Ok(n) if n > 0 => {
//         //                 let mut bufs = self.bufs.lock().unwrap();
//         //                 let buffer = bufs.get_mut(addr).unwrap();
//         //                 buffer.extend_from_slice(&buf[..n]);

//         //                 let mut offset = 0;
//         //                 while buffer.len() >= offset + 4 {
//         //                     let len =
//         //                         u32::from_be_bytes(buffer[offset..offset + 4].try_into().unwrap())
//         //                             as usize;
//         //                     if buffer.len() < offset + 4 + len {
//         //                         break;
//         //                     }
//         //                     let mut frame = (len as u32).to_be_bytes().to_vec();
//         //                     frame.extend_from_slice(&buffer[offset + 4..offset + 4 + len]);
//         //                     offset += 4 + len;

//         //                     let msg: NetworkMessage = self.serializer.deserialize(frame).unwrap();
//         //                     replies.push(msg);
//         //                 }

//         //                 if offset > 0 {
//         //                     buffer.drain(0..offset);
//         //                 }
//         //             }
//         //             _ => {}
//         //         }
//         //     }
//         // }

//         replies
//     }
// }

// // Implement Clone so the client can be passed into async tasks.
// impl Clone for NativeTcpClient {
//     fn clone(&self) -> Self {
//         Self {
//             conns: Arc::clone(&self.conns),
//             bufs: Arc::clone(&self.bufs),
//             serializer: self.serializer.clone(),
//         }
//     }
// }

use std::collections::HashMap;
use std::io::Write;
use std::net::TcpStream;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::bindings::paxos::default::network_types::NetworkMessage;
use crate::bindings::paxos::default::paxos_types::Node;
use crate::serializer::{MySerializer, Serializer};

pub struct NativeTcpClient {
    tx: Sender<(String, Vec<u8>)>,
}

impl NativeTcpClient {
    pub fn new() -> Self {
        println!("[Client] Starting TCP client thread");
        let (tx, rx) = mpsc::channel::<(String, Vec<u8>)>();
        let serializer = MySerializer;
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
        let serializer = MySerializer;
        let msg_bytes = serializer.serialize(message);

        for node in nodes {
            let addr = node.address;
            let _ = self.tx.send((addr, msg_bytes.clone()));
        }
    }

    pub fn send_message(&self, nodes: Vec<Node>, message: NetworkMessage) -> Vec<NetworkMessage> {
        let serializer = MySerializer;
        let msg_bytes = serializer.serialize(message);
        let mut replies = vec![];

        // for node in nodes {
        //     let addr = node.address;
        //     let _ = self.tx.send((addr, msg_bytes.clone()));
        //     // Here we would normally wait for a reply, but this is a fire-and-forget example.
        // }

        replies
    }
}
