use crate::paxos_bindings::{self, MessagePayloadExt as _};
use crate::{paxos_wasm::PaxosWasmtime, translation_layer::convert_internal_state_to_proto};
use paxos_bindings::paxos::default::paxos_types::{ClientRequest, Value};
use proto::paxos_proto;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tonic::{Request, Response, Status};
use tracing::info;

#[derive(Clone)]
pub struct PaxosService {
    pub paxos_wasmtime: Arc<PaxosWasmtime>,

    pub client_seq: Arc<AtomicU32>, // TODO: Fix this hack
}

pub async fn create_clients(
    endpoints: Vec<String>,
) -> Arc<Mutex<Vec<paxos_proto::paxos_client::PaxosClient<tonic::transport::Channel>>>> {
    let mut clients_vec = Vec::new();
    for endpoint in endpoints {
        let full_endpoint = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            endpoint
        } else {
            format!("http://{}", endpoint)
        };
        let client = paxos_proto::paxos_client::PaxosClient::connect(full_endpoint)
            .await
            .unwrap();
        clients_vec.push(client);
    }
    Arc::new(Mutex::new(clients_vec))
}

#[tonic::async_trait]
impl paxos_proto::paxos_server::Paxos for PaxosService {
    async fn propose_value(
        &self,
        request: Request<paxos_proto::ProposeRequest>,
    ) -> Result<Response<paxos_proto::ProposeResponse>, Status> {
        info!("GRPC propose_value: start");
        let req = request.into_inner();
        let mut store = self.paxos_wasmtime.store.lock().await;
        let resource = self.paxos_wasmtime.resource();

        // TODO: Have a standardize way to do this conversion. Have it per user.
        let seq: u32 = self.client_seq.fetch_add(1, Ordering::Relaxed) + 1;

        let request: ClientRequest = ClientRequest {
            client_id: "Client 1".into(),
            client_seq: seq,
            value: Value {
                is_noop: false,
                command: Some(req.value),
            },
        };

        let success = resource
            .call_run_paxos(&mut *store, self.paxos_wasmtime.resource_handle, &request)
            .await
            .map_err(|e| Status::internal(format!("Propose failed: {:?}", e)))?;

        info!("GRPC propose_value: finish (success: {})", success);
        Ok(Response::new(paxos_proto::ProposeResponse { success }))
    }

    // TODO: Make it an option to not send responses back
    async fn deliver_message(
        &self,
        request: Request<paxos_proto::NetworkMessage>,
    ) -> Result<Response<paxos_proto::NetworkMessage>, Status> {
        info!("GRPC deliver_message: start");
        let proto_msg = request.into_inner();
        let wit_msg = paxos_bindings::paxos::default::network::NetworkMessage::try_from(proto_msg)
            .map_err(|e| Status::invalid_argument(format!("Invalid network message: {}", e)))?;

        let mut store = self.paxos_wasmtime.store.lock().await;
        let resource = self.paxos_wasmtime.resource();

        let internal_response = resource
            .call_handle_message(&mut *store, self.paxos_wasmtime.resource_handle, &wit_msg)
            .await
            .map_err(|e| Status::internal(format!("Deliver message failed: {:?}", e)))?;

        info!(
            "GRPC deliver_message: finish payload type: {}",
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
