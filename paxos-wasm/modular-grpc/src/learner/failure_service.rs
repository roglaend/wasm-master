use proto::paxos_proto::{self, network_message::Payload};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tonic::Status;
use tonic::transport::Channel;

use crate::learner::paxos_bindings::paxos::default::paxos_types::Node;
use crate::learner::paxos_wasm::PaxosWasmtime;

#[derive(Clone)]
pub struct FailureService {
    pub paxos_wasmtime: Arc<PaxosWasmtime>,
}

impl FailureService {
    pub fn start_heartbeat_sender(
        &self,
        node: Node,
        remote_nodes: Vec<Node>,
        heartbeat_interval: Duration,
    ) {
        // Plain rust heartbeat sender to gRPC endpoints
        for remote_node in remote_nodes {
            // TODO: Find a way to better synchronize the tasks sending heartbeats, as this can cause them to get out of sync (which might or might be beneficial?)
            let node_clone = node.clone();
            tokio::spawn(async move {
                let channel = loop {
                    match Channel::from_shared(format!("http://{}", remote_node.address))
                        .unwrap()
                        .connect()
                        .await
                    {
                        Ok(channel) => break channel,
                        Err(_) => {
                            // TODO : This helps eventually getting connected in start up, and possibly reconnect nodes if they come back up
                            sleep(Duration::from_secs(5)).await;
                        }
                    }
                };
                let mut client = paxos_proto::paxos_client::PaxosClient::new(channel);
                loop {
                    let payload = Payload::Heartbeat(paxos_proto::HeartbeatPayload {
                        sender: Some(node_clone.clone().into()), // TODO: we already have sender in the NetworkMessage... *Shrug*
                        timestamp: 0, // TODO: Set this properly if we need it.
                    });

                    let proto_msg = paxos_proto::NetworkMessage {
                        sender: Some(node_clone.clone().into()),
                        payload: Some(payload),
                    };

                    let req = tonic::Request::new(proto_msg.clone());

                    match client.deliver_message(req).await {
                        Ok(_) => {
                            // println!("Heartbeat sent to {}", addr);
                        }
                        Err(_) => {
                            // sleep(Duration::from_secs(5)).await;
                        }
                    }
                    sleep(heartbeat_interval).await;
                }
            });
        }
    }

    pub fn start_failure_service(self: Arc<Self>, interval: Duration) {
        // TODO : Update sleep time based on response from component
        let _sleep_time = interval;

        tokio::spawn(async move {
            sleep(interval).await;
            loop {
                sleep(interval).await;
                let mut store = self.paxos_wasmtime.store.lock().await;
                let resource = self.paxos_wasmtime.resource();

                // Calls the paxos coordinator to check for stale nodes
                // let _ = resource
                //     .call_failure_service(&mut *store, self.paxos_wasmtime.resource_handle)
                //     .await
                //     .map_err(|e| {
                //         Status::internal(format!(
                //             "[Failure Service] Failed to call wasm component to check for state nodes: {:?}",
                //             e
                //         ))
                //     });
            }
        });
    }
}
