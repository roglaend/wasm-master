use crate::paxos_bindings::{self, MessagePayloadExt as _};
use crate::{paxos_wasm::PaxosWasmtime, translation_layer::convert_internal_state_to_proto};
use futures::future::join_all;
use paxos_bindings::paxos::default::paxos_types::{ClientRequest, Value};
use proto::paxos_proto;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tonic::{Request, Response, Status};
use tracing::{debug, info};

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
        let mut store = self.paxos_wasmtime.store.lock().await;
        let resource = self.paxos_wasmtime.resource();

        // TODO: Have a standardize way to do this conversion. Have it per user.
        let seq: u32 = self.client_seq.fetch_add(1, Ordering::Relaxed) + 1;

        let client_request: ClientRequest = ClientRequest {
            client_id: "Client 1".into(),
            client_seq: seq,
            value: Value {
                is_noop: false,
                command: Some(req.value),
            },
        };

        let success = resource
            .call_submit_client_request(
                &mut *store,
                self.paxos_wasmtime.resource_handle,
                &client_request,
            )
            .await
            .map_err(|e| Status::internal(format!("Failed to submit client request: {:?}", e)))?;

        info!(
            "[gRPC Service] Finished submitting client request, success: {}).",
            success
        );
        Ok(Response::new(paxos_proto::ProposeResponse { success }))
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

    //* Old way to run a single synchronous paxos instance. */
    // async fn run_paxos_instance_sync(
    //     &self,
    //     request: Request<paxos_proto::ProposeRequest>,
    // ) -> Result<Response<paxos_proto::ProposeResponse>, Status> {
    //     info!(
    //         "[gRPC Service] Starting process to run a single instance of the paxos algorithm synchronously."
    //     );
    //     let req = request.into_inner();
    //     let mut store = self.paxos_wasmtime.store.lock().await;
    //     let resource = self.paxos_wasmtime.resource();

    //     // TODO: Have a standardize way to do this conversion. Have it per user.
    //     let seq: u32 = self.client_seq.fetch_add(1, Ordering::Relaxed) + 1;

    //     let request: ClientRequest = ClientRequest {
    //         client_id: "Client 1".into(),
    //         client_seq: seq,
    //         value: Value {
    //             is_noop: false,
    //             command: Some(req.value),
    //         },
    //     };

    //     let success = resource
    //         .run_paxos_instance_sync(&mut *store, self.paxos_wasmtime.resource_handle, &req)
    //         .await
    //         .map_err(|e| Status::internal(format!("Failed to submit client request: {:?}", e)))?;

    //     info!(
    //         "[gRPC Service] Finished running the single synchronous instance of paxos algorithm, success: {}).",
    //         success
    //     );
    //     Ok(Response::new(paxos_proto::ProposeResponse { success }))
    // }

    async fn get_value(
        &self,
        _request: Request<paxos_proto::Empty>,
    ) -> Result<Response<paxos_proto::GetValueResponse>, Status> {
        info!("GRPC get_value: start");
        let mut store = self.paxos_wasmtime.store.lock().await;
        let resource = self.paxos_wasmtime.resource();

        let value = resource
            .call_get_learned_value(&mut *store, self.paxos_wasmtime.resource_handle)
            .await
            .map_err(|e| Status::internal(format!("Get value failed: {:?}", e)))?;

        // TODO: This is wrong. Should instead get a "ClientResponse" instead of a "Value"
        let proto_value = paxos_proto::Value {
            is_noop: value.is_noop,
            command: value.command.unwrap_or_default(),
        };

        let response = paxos_proto::GetValueResponse {
            value: Some(proto_value),
        };
        info!("GRPC get_value: finish (value: {:?})", response.value);
        Ok(Response::new(response))
    }

    async fn get_state(
        &self,
        _request: Request<paxos_proto::Empty>,
    ) -> Result<Response<paxos_proto::PaxosState>, Status> {
        info!("GRPC get_state: start");
        let mut store = self.paxos_wasmtime.store.lock().await;
        let resource = self.paxos_wasmtime.resource();

        let internal_state = resource
            .call_get_state(&mut *store, self.paxos_wasmtime.resource_handle)
            .await
            .map_err(|e| Status::internal(format!("Get state failed: {:?}", e)))?;

        let proto_state = convert_internal_state_to_proto(internal_state);
        info!("GRPC get_state: finish");
        Ok(Response::new(proto_state))
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
