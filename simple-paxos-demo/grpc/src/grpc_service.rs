use crate::paxos_wasm::PaxosWasmtime;
use log::info;
use proto::paxos_proto;
use std::sync::{Arc, Mutex};
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct PaxosService {
    pub paxos_wasmtime: Arc<PaxosWasmtime>,
}

pub async fn create_clients(
    endpoints: Vec<String>,
) -> Arc<Mutex<Vec<paxos_proto::paxos_client::PaxosClient<tonic::transport::Channel>>>> {
    // Build gRPC clients for remote nodes.
    let mut clients_vec = Vec::new();
    for endpoint in endpoints {
        // Prepend "http://" if missing.
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
        info!("GRPC SERVICE: called propose_value");
        let req = request.into_inner();
        let mut store = self.paxos_wasmtime.store.lock().await;
        let resource = self.paxos_wasmtime.resource();
        let success = resource
            .call_run_paxos(&mut *store, self.paxos_wasmtime.resource_handle, &req.value)
            .await
            .map_err(|e| Status::internal(format!("Propose failed: {:?}", e)))?;
        Ok(Response::new(paxos_proto::ProposeResponse { success }))
    }

    async fn deliver_message(
        &self,
        request: Request<paxos_proto::DeliverMessageRequest>,
    ) -> Result<Response<paxos_proto::DeliverMessageResponse>, Status> {
        info!("GRPC SERVICE: called deliver_message");
        let req = request.into_inner();
        let mut store = self.paxos_wasmtime.store.lock().await;
        let resource = self.paxos_wasmtime.resource();
        let success = resource
            .call_deliver_message(
                &mut *store,
                self.paxos_wasmtime.resource_handle,
                &req.message,
            )
            .await
            .map_err(|e| Status::internal(format!("Deliver message failed: {:?}", e)))?;
        Ok(Response::new(paxos_proto::DeliverMessageResponse {
            success,
        }))
    }

    async fn get_value(
        &self,
        _request: Request<paxos_proto::Empty>,
    ) -> Result<Response<paxos_proto::GetValueResponse>, Status> {
        let mut store = self.paxos_wasmtime.store.lock().await;
        let resource = self.paxos_wasmtime.resource();
        let value = resource
            .call_get_learned_value(&mut *store, self.paxos_wasmtime.resource_handle)
            .await
            .map_err(|e| Status::internal(format!("Get value failed: {:?}", e)))?;

        let response = paxos_proto::GetValueResponse {
            value: value.unwrap_or_default(),
        };
        Ok(Response::new(response))
    }
}
