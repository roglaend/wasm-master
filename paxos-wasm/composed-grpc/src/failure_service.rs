use proto::paxos_proto::{self, network_message::Payload};
use std::time::Duration;
use tonic;

use std::{
    sync::Arc,
    time::SystemTime,
};

use tokio::time::sleep;
use tonic::Status;

use tonic::transport::Channel;

use crate::paxos_wasm::PaxosWasmtime;

#[derive(Clone)]
pub struct FailureService {
    pub paxos_wasmtime: Arc<PaxosWasmtime>,
}

impl FailureService {
    pub fn start_heartbeat_sender(
        &self,
        node_id: u64,
        endpoints: Vec<String>,
        heartbeat_interval: Duration
    ) {
        // Plain rust heartbeat sender to gRPC endpoints
        for addr in endpoints {
            let node_id_clone = node_id.clone();
            tokio::spawn(async move {
                let channel = loop {
                    match Channel::from_shared(format!("http://{}", addr))
                    .unwrap()
                    .connect()
                    .await {
                        Ok(channel) => break channel,
                        Err(_) => {
                            // TODO : This helps eventually getting connceted in start up, and possibly recconect nodes if they come back up
                            sleep(Duration::from_secs(5)).await;
                        }
                    }
                };
                let mut client = paxos_proto::paxos_client::PaxosClient::new(channel);
                loop {
                    let payload = Payload::Heartbeat(paxos_proto::HeartbeatPayload { 
                        nodeid: node_id_clone,
                        });

                    let proto_msg = paxos_proto::NetworkMessage {
                        kind: 5,
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
            }
        );
        }
    }

    pub fn start_failure_service(self: Arc<Self>, interval: Duration) {
        // TODO : Update sleep time based on reponse from component
        let _sleep_time = interval;
        tokio::spawn(async move {
            loop {
                sleep(interval).await;
                let mut store = self.paxos_wasmtime.store.lock().await;
                let resource = self.paxos_wasmtime.resource();

                // Calls the paxos coordinator to check for stale nodes
                let _ = resource
                    .call_failure_service(&mut *store, self.paxos_wasmtime.resource_handle)
                    .await
                    .map_err(|e| Status::internal(format!("Deliver message failed: {:?}", e)));
                
            }
        });

    }

}