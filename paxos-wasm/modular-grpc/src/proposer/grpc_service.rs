use crate::proposer::paxos_bindings::{self, MessagePayloadExt as _};
use crate::proposer::paxos_wasm::PaxosWasmtime;
use futures::future::join_all;
use paxos_bindings::paxos::default::paxos_types::{ClientRequest, Value};
use proto::paxos_proto;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};
// use std::time::Duration;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use tokio::sync::oneshot;
use tonic::{Request, Response, Status};
use tracing::{debug, info};

use super::paxos_bindings::paxos::default::paxos_types::ClientResponse;

pub static RESPONSE_REGISTRY: Lazy<DashMap<u64, oneshot::Sender<ClientResponse>>> =
    Lazy::new(|| DashMap::new());

// pub static LATENCY_START_TIMES: Lazy<DashMap<u64, Instant>> =
//     Lazy::new(|| DashMap::new());

// /// A global map to store computed latencies.
// pub static LATENCIES_INSIDE_WASM: Lazy<DashMap<u64, Duration>> =
//     Lazy::new(|| DashMap::new());

#[derive(Clone)]
pub struct PaxosService {
    pub paxos_wasmtime: Arc<PaxosWasmtime>,

    pub client_seq: Arc<AtomicU32>, // TODO: Fix this hack, having proper control of this and other user specific data.
}

pub async fn create_clients(
    endpoints: Vec<String>,
) -> Arc<Mutex<Vec<paxos_proto::paxos_client::PaxosClient<tonic::transport::Channel>>>> {
    // Create a vector of connection futures.
    let connection_futures = endpoints.into_iter().map(|endpoint| {
        let full_endpoint = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            endpoint
        } else {
            format!("http://{}", endpoint)
        };
        async move {
            paxos_proto::paxos_client::PaxosClient::connect(full_endpoint)
                .await
                .ok()
        }
    });

    // Run all connection futures concurrently.
    let results = join_all(connection_futures).await;

    // Filter out failed connections. // TODO: Handle this properly or just log it?
    let clients_vec: Vec<_> = results.into_iter().filter_map(|client| client).collect();

    Arc::new(Mutex::new(clients_vec))
}
#[tonic::async_trait]
impl paxos_proto::paxos_server::Paxos for PaxosService {
    async fn propose_value(
        &self,
        request: Request<paxos_proto::ProposeRequest>,
    ) -> Result<Response<paxos_proto::ProposeResponse>, Status> {
        info!("[gRPC Service] Starting process to submit client request to wasm component.");
        let req = request.into_inner();

        // TODO: Have a standardize way to do this conversion. Have it per user.
        // let seq: u32 = self.client_seq.fetch_add(1, Ordering::Relaxed) + 1;

        let value = Value {
            is_noop: false,
            command: Some(req.value),
            client_id: req.client_id,
            client_seq: req.client_seq,
        };

        let request_id = req.client_id * 1000 + req.client_seq;
        let (response_tx, response_rx) = oneshot::channel();
        RESPONSE_REGISTRY.insert(request_id.clone(), response_tx);

        // LATENCY_START_TIMES.insert(request_id.clone(), Instant::now());

        {
            let mut store = self.paxos_wasmtime.store.lock().await;
            let resource = self.paxos_wasmtime.resource();
            let success = resource
                .call_submit_client_request(
                    &mut *store,
                    self.paxos_wasmtime.resource_handle,
                    &value,
                )
                .await
                .map_err(|e| {
                    Status::internal(format!("Failed to submit client request: {:?}", e))
                })?;

            info!(
                "[gRPC Service] Finished submitting client request, success: {}).",
                success
            );
        }

        let result = response_rx.await.map_err(|e| {
            info!(
                "[gRPC Service] Failed to receive response for client_id {}: {:?}",
                request_id, e
            );
            Status::internal(format!(
                "Failed to receive response for client_id {}: {:?}",
                request_id, e
            ))
        })?;

        let result_message = &format!("{:?}", result.command_result);

        Ok(Response::new(paxos_proto::ProposeResponse {
            success: true,
            message: result_message.to_string(),
        }))
    }

    // TODO: Make it an option to not send responses back
    async fn deliver_message(
        &self,
        request: Request<paxos_proto::NetworkMessage>,
    ) -> Result<Response<paxos_proto::NetworkMessage>, Status> {
        debug!("[gRPC Service] Starting process to deliver network message to wasm component.");
        let proto_msg = request.into_inner();
        let wit_msg = paxos_bindings::paxos::default::network::NetworkMessage::try_from(proto_msg)
            .map_err(|e| Status::invalid_argument(format!("Invalid network message: {}", e)))?;

        let mut store = self.paxos_wasmtime.store.lock().await;
        let resource = self.paxos_wasmtime.resource();

        let internal_response = resource
            .call_handle_message(&mut *store, self.paxos_wasmtime.resource_handle, &wit_msg)
            .await
            .map_err(|e| {
                Status::internal(format!("Failed to handle delivered message: {:?}", e))
            })?;

        debug!(
            "[gRPC Service] Finished finished delivering message to wasm component, payload type: {}.",
            internal_response.payload.payload_type()
        );

        // Convert our internal response to a proto response.
        let proto_response: paxos_proto::NetworkMessage = internal_response.into();
        Ok(Response::new(proto_response))
    }

    async fn get_value(
        &self,
        _request: Request<paxos_proto::Empty>,
    ) -> Result<Response<paxos_proto::GetValueResponse>, Status> {
        todo!()
    }
    async fn get_state(
        &self,
        _request: Request<paxos_proto::Empty>,
    ) -> Result<Response<paxos_proto::PaxosState>, Status> {
        todo!()
    }

    async fn get_logs(
        &self,
        request: Request<paxos_proto::GetLogsRequest>,
    ) -> Result<Response<paxos_proto::GetLogsResponse>, Status> {
        info!("GRPC get_logs: start");
        let req = request.into_inner();
        let last_offset = req.last_offset;
        let store = self.paxos_wasmtime.store.lock().await;

        let (entries, new_offset) = store.data().logger.get_logs(last_offset);

        let proto_entries: Vec<paxos_proto::LogEntry> = entries
            .into_iter()
            .map(|(offset, message)| paxos_proto::LogEntry { offset, message })
            .collect();
        info!(
            "GRPC get_logs: finish (returning {} entries, new offset {})",
            proto_entries.len(),
            new_offset
        );
        Ok(Response::new(paxos_proto::GetLogsResponse {
            entries: proto_entries,
            new_offset,
        }))
    }
}
