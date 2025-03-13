use core::num;
use std::env;
use log::info;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status, transport::Server};
use wasmtime::component::{Component, Linker, ResourceAny};
use wasmtime::{Engine, Result, Store};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};
use serde::{Serialize, Deserialize};
use serde_json;

// Assume we have a generated gRPC proto in paxos.proto:
pub mod paxos_proto {
    tonic::include_proto!("paxos");
}

use paxos_proto::paxos_server::{Paxos, PaxosServer};
use paxos_proto::{DeliverMessageRequest, DeliverMessageResponse, ProposeRequest, ProposeResponse};

mod proposer_bindings {
    wasmtime::component::bindgen! {{
        path: "../shared/wit/paxos.wit",
        world: "proposer-world",
    }}
}
mod acceptor_bindings {
    wasmtime::component::bindgen! {{
        path: "../shared/wit/paxos.wit",
        world: "acceptor-world",
    }}
}
mod learner_bindings {
    wasmtime::component::bindgen! {{
        path: "../shared/wit/paxos.wit",
        world: "learner-world",
    }}
}
mod kv_store_bindings {
    wasmtime::component::bindgen! {{
        path: "../shared/wit/paxos.wit",
        world: "kv-store-world",
    }}
}
mod paxos_bindings {
    wasmtime::component::bindgen! {{
        path: "../shared/wit/paxos.wit",
        world: "paxos-world",
    }}
}


use utils::ComponentRunStates;


/// A simple message envelope to carry Paxos phase messages.
/// For prepare, the `value` is None; for accept/commit, it must be Some.
#[derive(Serialize, Deserialize)]
enum PaxosMessageType {
    Prepare,
    Accept,
    Commit,
}

#[derive(Serialize, Deserialize)]
struct PaxosMessage {
    message_type: PaxosMessageType,
    slot: u64,
    ballot: u64,
    value: Option<String>,
}

pub struct PaxosWasmtime {
    engine: Engine,
    store: Store<ComponentRunStates>,
    bindings: paxos_bindings::PaxosWorld,
    resource_handle: ResourceAny,
}

impl PaxosWasmtime {
    pub fn new(node_id: u64, num_nodes: u64) -> Result<Self> {
        let engine = Engine::default();
        let state = ComponentRunStates::new();
        let mut store = Store::new(&engine, state);
        let mut linker = Linker::<ComponentRunStates>::new(&engine);

        wasmtime_wasi::add_to_linker_sync(&mut linker)?;

        // TODO: Make it work to compose at runtime. Else, use wac.
        // let proposer_component = Component::from_file(
        //     &engine,
        //     "../../target/wasm32-wasip2/release/proposer_grpc.wasm",
        // )?;
        // let acceptor_component = Component::from_file(
        //     &engine,
        //     "../../target/wasm32-wasip2/release/acceptor-grpc.wasm",
        // )?;
        // let learner_component = Component::from_file(
        //     &engine,
        //     "../../target/wasm32-wasip2/release/learner_grpc.wasm",
        // )?;
        // let kv_store_component = Component::from_file(
        //     &engine,
        //     "../../target/wasm32-wasip2/release/kv-store-grpc.wasm",
        // )?;
        let original_paxos_component = Component::from_file(
            &engine,
            "../../target/wasm32-wasip2/release/paxos-grpc.wasm",
        )?;
        let composed_paxos_component = Component::from_file(
            &engine,
            "../../target/wasm32-wasip2/release/composed-paxos-grpc.wasm",
        )?;

        let test = paxos_bindings::PaxosWorld::instantiate(
            &mut store,
            &original_paxos_component,
            &linker,
        )?;

        let _test_bindings = test.paxos_default_paxos_coordinator();

        let final_bindings = paxos_bindings::PaxosWorld::instantiate(
            &mut store,
            &composed_paxos_component,
            &linker,
        )?;

        let paxos_guest = final_bindings.paxos_default_paxos_coordinator();
        let paxos_resource = paxos_guest.paxos_coordinator_resource();
        let paxos_resource_handle =
            paxos_resource.call_constructor(&mut store, node_id, num_nodes)?;

        Ok(Self {
            engine: engine,
            store: store,
            bindings: final_bindings,
            resource_handle: paxos_resource_handle,
        })
    }

    // Helper method to access the guest from the bindings.
    fn guest<'a>(
        &'a self,
    ) -> &'a paxos_bindings::exports::paxos::default::paxos_coordinator::Guest {
        self.bindings.paxos_default_paxos_coordinator()
    }

    // Similarly, a helper to access the resource.
    fn resource<'a>(
        &'a self,
    ) -> paxos_bindings::exports::paxos::default::paxos_coordinator::GuestPaxosCoordinatorResource<'a>
    {
        self.guest().paxos_coordinator_resource()
    }

}

#[derive(Clone)]
pub struct PaxosHostService {
    paxos_wasmtime: Arc<PaxosWasmtime>,

    clients: Arc<Mutex<Vec<paxos_proto::paxos_client::PaxosClient<tonic::transport::Channel>>>>,
}

impl PaxosHostService {
    /// Helper to broadcast a PaxosMessage (serialized as JSON) to all remote nodes.
    async fn broadcast_message(&self, message: &PaxosMessage) -> Vec<bool> {
        let json = serde_json::to_string(message).unwrap();
        let clients = self.clients.lock().await;
        let mut results = vec![];
        for client in clients.iter() {
            let deliver_req = tonic::Request::new(DeliverMessageRequest {
                message: json.clone(),
            });
            match client.clone().deliver_message(deliver_req).await {
                Ok(resp) => results.push(resp.into_inner().success),
                Err(_) => results.push(false),
            }
        }
        results
    }
}

#[tonic::async_trait]
impl Paxos for PaxosHostService {
    fn new (&self,)


    async fn propose_value(
        &self,
        request: Request<ProposeRequest>,
    ) -> Result<Response<ProposeResponse>, Status> {
        let req = request.into_inner();

        let mut store = self.paxos_wasmtime.store.lock().await;
        let resource = self.paxos_wasmtime.resource();
        let resource_handle = self.paxos_wasmtime.resource_handle;
        let proposal_value = req.value;

        println!("Proposing Value: {}", proposal_value);

        let success = resource.call_propose(&mut *store, resource_handle, &proposal_value).unwrap();

        Ok(Response::new(ProposeResponse { success }))
    }

    async fn deliver_message(
        &self,
        request: Request<DeliverMessageRequest>,
    ) -> Result<Response<DeliverMessageResponse>, Status> {
        let req = request.into_inner();
        let success = self.paxos_wasmtime.store.
            .lock()
            .await
            .deliver_message(req.message)
            .await;
        Ok(Response::new(DeliverMessageResponse { success }))
    }

    async fn get_value(&self, _request: Request<()>) -> Result<Response<ProposeResponse>, Status> {
        let value = self.coordinator.lock().await.get().await;
        // Here, we return success if a value is learned.
        Ok(Response::new(ProposeResponse {
            success: value.is_some(),
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging.
    env_logger::init();

    let node_id: u64 = env::args()
        .nth(1)
        .unwrap_or_else(|| "1".to_string())
        .parse()
        .expect("Invalid node ID (must be an integer).");

    // Assume node_id 1 is the leader, and there are 3 nodes.
    // Also assume remote endpoints for the other coordinators.

    let remote_endpoints = vec!["127.0.0.1:50052".to_string(), "127.0.0.1:50053".to_string(), "127.0.0.1:50054".to_string()];

    let paxos_wasmtime = PaxosWasmtime::new(node_id, remote_endpoints.len() as u64);

    // Set up gRPC server.
    let addr = "[::1]:50051".parse()?;
    let paxos_service = PaxosHostService { paxos_wasmtime,  };
    info!("gRPC server listening on {}", addr);
    Server::builder()
        .add_service(PaxosServer::new(paxos_service))
        .serve(addr)
        .await?;

    Ok(())
}

