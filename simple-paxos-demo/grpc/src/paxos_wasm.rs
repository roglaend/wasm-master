use crate::grpc_service::create_clients;
use futures::future::join_all;
use log::info;
use proto::paxos_proto;
use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::Mutex;
use utils::ComponentRunStates;
use wasmtime::component::{Component, Linker, ResourceAny};
use wasmtime::{Engine, Store};

pub mod paxos_bindings {
    wasmtime::component::bindgen! {{
        path: "../shared/wit/paxos.wit",
        world: "paxos-world",
        async: true,
        // TODO: Try async again later
    }}
}

pub struct PaxosWasmtime {
    pub _engine: Engine,
    pub store: Mutex<Store<ComponentRunStates>>,
    pub bindings: paxos_bindings::PaxosWorld,
    pub resource_handle: ResourceAny,
}

impl PaxosWasmtime {
    pub async fn new(
        node_id: u64,
        nodes: Vec<paxos_bindings::paxos::default::network::Node>,
    ) -> Result<Self, Box<dyn Error>> {
        let mut config = wasmtime::Config::default();
        config.async_support(true);
        let engine = Engine::new(&config)?;

        let state = ComponentRunStates::new();
        let mut store = Store::new(&engine, state);
        let mut linker = Linker::<ComponentRunStates>::new(&engine);

        wasmtime_wasi::add_to_linker_async(&mut linker)?;

        paxos_bindings::paxos::default::network::add_to_linker(&mut linker, |s| s)?;

        let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("Must have a parent")
            .to_owned();

        // Load WASM components.
        // let original_component = Component::from_file(
        //     &engine,
        //     "../../target/wasm32-wasip2/release/paxos_grpc.wasm",
        // )?;
        let composed_component = Component::from_file(
            &engine,
            workspace_dir.join("target/wasm32-wasip2/release/composed_paxos_grpc.wasm"),
        )?;

        // Instantiate original if necessary.
        // let _ =
        //     paxos_bindings::PaxosWorld::instantiate_async(&mut store, &original_component, &linker)
        //         .await?;

        let final_bindings =
            paxos_bindings::PaxosWorld::instantiate_async(&mut store, &composed_component, &linker)
                .await?;
        let paxos_guest = final_bindings.paxos_default_paxos_coordinator();
        let paxos_resource = paxos_guest.paxos_coordinator_resource();
        let resource_handle = paxos_resource
            .call_constructor(&mut store, node_id, &nodes)
            .await?;

        // paxos_bindings::PaxosWorld::add_to_linker(&mut linker, |s| s)?;

        Ok(Self {
            _engine: engine,
            store: Mutex::new(store),
            bindings: final_bindings,
            resource_handle,
        })
    }

    // Helper methods to access WASM guest.
    pub fn guest<'a>(
        &'a self,
    ) -> &'a paxos_bindings::exports::paxos::default::paxos_coordinator::Guest {
        self.bindings.paxos_default_paxos_coordinator()
    }

    pub fn resource<'a>(
        &'a self,
    ) -> paxos_bindings::exports::paxos::default::paxos_coordinator::GuestPaxosCoordinatorResource<'a>
    {
        self.guest().paxos_coordinator_resource()
    }
}

impl paxos_bindings::paxos::default::network::Host for ComponentRunStates {
    async fn send_hello(&mut self) -> String {
        return "Hello".to_string();
    }

    async fn send_message(
        &mut self,
        nodes: Vec<paxos_bindings::paxos::default::network::Node>,
        message: paxos_bindings::paxos::default::network::NetworkMessage,
    ) -> Vec<paxos_bindings::paxos::default::network::NetworkResponse> {
        // Convert the variant payload into a string representation.
        let payload_str = match &message.payload {
            paxos_bindings::paxos::default::network::MessagePayload::Prepare(payload) => {
                format!("slot: {}, ballot: {}", payload.slot, payload.ballot)
            }
            paxos_bindings::paxos::default::network::MessagePayload::Accept(payload) => {
                format!(
                    "slot: {}, ballot: {}, proposal: {}",
                    payload.slot, payload.ballot, payload.proposal
                )
            }
            paxos_bindings::paxos::default::network::MessagePayload::Commit(payload) => {
                format!("slot: {}, value: {}", payload.slot, payload.value)
            }
            paxos_bindings::paxos::default::network::MessagePayload::Deliver(payload) => {
                // For a deliver payload, use the contained data.
                payload.data.clone()
            }
            paxos_bindings::paxos::default::network::MessagePayload::Promise(payload) => {
                format!(
                    "slot: {}, ballot: {}, accepted_ballot: {}, accepted_value: {:?}",
                    payload.slot, payload.ballot, payload.accepted_ballot, payload.accepted_value
                )
            }
            paxos_bindings::paxos::default::network::MessagePayload::Accepted(payload) => {
                format!(
                    "slot: {}, ballot: {}, accepted: {}",
                    payload.slot, payload.ballot, payload.accepted
                )
            }
            paxos_bindings::paxos::default::network::MessagePayload::Heartbeat(payload) => {
                format!("timestamp: {}", payload.timestamp)
            }
        };

        // Log the message details using the string representation.
        match message.kind {
            paxos_bindings::paxos::default::network::NetworkMessageKind::Prepare => {
                info!("Sending a prepare message with payload: {}", payload_str);
            }
            paxos_bindings::paxos::default::network::NetworkMessageKind::Accept => {
                info!("Sending an accept message with payload: {}", payload_str);
            }
            paxos_bindings::paxos::default::network::NetworkMessageKind::Commit => {
                info!("Sending a commit message with payload: {}", payload_str);
            }
            paxos_bindings::paxos::default::network::NetworkMessageKind::Deliver => {
                info!("Sending a deliver message with payload: {}", payload_str);
            }
            paxos_bindings::paxos::default::network::NetworkMessageKind::Promise => {
                info!("Sending a promise message with payload: {}", payload_str);
            }
            paxos_bindings::paxos::default::network::NetworkMessageKind::Accepted => {
                info!("Sending an accepted message with payload: {}", payload_str);
            }
            paxos_bindings::paxos::default::network::NetworkMessageKind::Heartbeat => {
                info!("Sending a heartbeat message with payload: {}", payload_str);
            }
        }

        // Extract endpoints.
        let endpoints: Vec<String> = nodes.into_iter().map(|node| node.address).collect();

        let responses = async {
            // Create gRPC clients.
            let clients_arc = create_clients(endpoints.clone()).await;
            // Acquire the lock asynchronously.
            let clients: Vec<_> = {
                let guard = clients_arc.lock();
                guard.unwrap().clone()
            };

            // Build futures for sending the message.
            let futures: Vec<_> = clients
                .into_iter()
                .map(|mut client| {
                    let req = tonic::Request::new(paxos_proto::DeliverMessageRequest {
                        message: payload_str.clone(),
                    });
                    async move { client.deliver_message(req).await }
                })
                .collect();

            // Wait for all futures with a timeout.
            tokio::time::timeout(Duration::from_secs(5), join_all(futures)).await
        }
        .await;

        info!("Responses retrieved!");

        // Build network responses.
        let network_responses: Vec<_> = match responses {
            Ok(results) => results
                .into_iter()
                .map(|res| {
                    let success = res.map(|r| r.into_inner().success).unwrap_or(false);
                    let status = if success {
                        paxos_bindings::paxos::default::network::StatusKind::Success
                    } else {
                        paxos_bindings::paxos::default::network::StatusKind::Failure
                    };
                    paxos_bindings::paxos::default::network::NetworkResponse {
                        kind: message.kind,
                        status,
                    }
                })
                .collect(),
            Err(_) => endpoints
                .into_iter()
                .map(
                    |_| paxos_bindings::paxos::default::network::NetworkResponse {
                        kind: message.kind,
                        status: paxos_bindings::paxos::default::network::StatusKind::Failure,
                    },
                )
                .collect(),
        };

        network_responses
    }
}
