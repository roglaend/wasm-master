use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tonic::Status;
use tracing::{debug, error, info};

use crate::paxos_bindings::paxos::default::paxos_types::ClientResponse;
use crate::paxos_wasm::PaxosWasmtime;

use crate::grpc_service::RESPONSE_REGISTRY;

#[derive(Clone)]
pub struct RunPaxosService {
    pub paxos_wasmtime: Arc<PaxosWasmtime>,
}

impl RunPaxosService {
    pub fn start_paxos_run_loop(self: Arc<Self>, interval: Duration) {
        tokio::spawn(async move {
            loop {
                sleep(interval).await;
                let mut store = self.paxos_wasmtime.store.lock().await;
                let resource = self.paxos_wasmtime.resource();
                debug!("[Run-Paxos Service] Calling wasm component to run a new paxos loop.");
                // Calls into the the paxos run loop at an interval.
                let result = resource
                    .call_handle_tick(&mut *store, self.paxos_wasmtime.resource_handle)
                    .await
                    .map_err(|e| Status::internal(format!("[Run-Paxos Service] Failed to call wasm component to run a new paxos loop: {:?}",
                            e)));
                match result {
                    Ok(value) => {
                        if let Some(vec) = value {
                            info!(
                                "[Run-Paxos Service] Recieved executed for value {:?}).",
                                vec
                            );
                            // let id: u64 = val.client_id.clone().parse().unwrap_or(0);
                            // let client_seq = val.client_seq;
                            // handle_response(id, client_seq, val);
                            for val in vec {
                                let id: u64 = val.client_id.clone().parse().unwrap_or(0);
                                let client_seq = val.client_seq;
                                handle_response(id, client_seq, val);
                            }
                        } else {
                            debug!("[gRPC Service] No value returned from wasm component.");
                        }
                    }
                    Err(e) => {
                        debug!(
                            "[Run-Paxos Service] Failed to call wasm component to run a new paxos loop: {:?}",
                            e
                        );
                    }
                }
            }
        });
    }
}

fn handle_response(client_id: u64, client_seq: u64, result: ClientResponse) {
    let request_id = client_id * 1000 + client_seq;
    if let Some((_, sender)) = RESPONSE_REGISTRY.remove(&request_id) {
        let _ = sender.send(result);
        info!("Sent result for request_id: {}", request_id);
    } else {
        error!("No active sender found for request_id: {}", request_id);
    }
}
