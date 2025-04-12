use crate::host_messenger::HostMessenger;
use crate::traits::{ComponentRunStates, PaxosBindings};
use paxos_wasm_bindings_types::paxos::network_types::NetworkMessage;
use paxos_wasm_bindings_types::paxos::paxos_types::{Node, RunConfig};
use paxos_wasm_bindings_types::{self, MessagePayloadExt};
use proto::paxos_proto;
use std::error::Error;
use std::path::PathBuf;
use tokio::sync::Mutex;

use wasmtime::component::{Component, Linker, ResourceAny};
use wasmtime::{Engine, Store};

pub struct PaxosWasmtime<B: PaxosBindings> {
    pub _engine: Engine,
    pub store: tokio::sync::Mutex<Store<ComponentRunStates>>,
    pub bindings: B,
    pub resource_handle: ResourceAny,
}

use crate::coordinator_bindings;

impl<B: PaxosBindings> PaxosWasmtime<B> {
    pub async fn new(
        node: B::Node,
        nodes: Vec<B::Node>,
        is_leader: bool,
        run_config: B::RunConfig,
    ) -> Result<Self, Box<dyn Error>> {
        let mut config = wasmtime::Config::default();
        config.async_support(true);
        let engine = Engine::new(&config)?;

        let state = ComponentRunStates::new(node.clone());
        let mut store = Store::new(&engine, state);
        let mut linker = Linker::<ComponentRunStates>::new(&engine);

        wasmtime_wasi::add_to_linker_async(&mut linker)?;
        coordinator_bindings::paxos::default::network::add_to_linker(&mut linker, |s| s)?;
        coordinator_bindings::paxos::default::logger::add_to_linker(&mut linker, |s| s)?;

        let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .ok_or("Workspace folder not found")?
            .to_owned();

        let component_path =
            workspace_dir.join("target/wasm32-wasip2/release/composed_paxos_coordinator.wasm");
        let composed_component = Component::from_file(&engine, component_path)?;

        let bindings = B::instantiate_async(&mut store, &composed_component, &linker).await?;
        let resource_handle = bindings
            .construct_resource(&mut store, &node, &nodes, is_leader, &run_config)
            .await?;

        Ok(Self {
            _engine: engine,
            store: tokio::sync::Mutex::new(store),
            bindings,
            resource_handle,
        })
    }

    pub fn guest(&self) -> &<B as PaxosBindings>::Guest {
        self.bindings.guest()
    }

    pub fn resource<'a>(&'a self) -> <B as PaxosBindings>::GuestResource<'a> {
        self.bindings.guest_resource()
    }
}

impl coordinator_bindings::paxos::default::network::Host for ComponentRunStates {
    async fn send_hello(&mut self) -> String {
        "Hello".to_string()
    }

    async fn send_message_forget(
        &mut self,
        nodes: Vec<coordinator_bindings::paxos::default::paxos_types::Node>,
        message: coordinator_bindings::paxos::default::network_types::NetworkMessage,
    ) -> () {
        let shared_wasm_msg: NetworkMessage = message.into();

        self.logger.log_info(format!(
            "[Host Messenger] Sending network message, fire-and-forget , with payload type: {}",
            shared_wasm_msg.payload.payload_type()
        ));

        let proto_msg: paxos_proto::NetworkMessage = shared_wasm_msg.clone().into();
        let endpoints: Vec<String> = nodes.into_iter().map(|node| node.address).collect();

        HostMessenger::send_message_forget(endpoints, proto_msg).await
    }

    async fn send_message(
        &mut self,
        nodes: Vec<coordinator_bindings::paxos::default::paxos_types::Node>,
        message: coordinator_bindings::paxos::default::network_types::NetworkMessage,
    ) -> Vec<coordinator_bindings::paxos::default::network_types::NetworkMessage> {
        let shared_wasm_msg: NetworkMessage = message.into();

        self.logger.log_info(format!(
            "[Host Messenger] Sending network message, and waiting for responses, with payload type: {}",
            shared_wasm_msg.payload.payload_type()
        ));

        let proto_msg: paxos_proto::NetworkMessage = shared_wasm_msg.into();

        let endpoints: Vec<String> = nodes.into_iter().map(|node| node.address).collect();

        let shared_msgs = HostMessenger::send_message(endpoints, proto_msg).await;

        let host_msgs: Vec<coordinator_bindings::paxos::default::network_types::NetworkMessage> =
            shared_msgs.into_iter().map(Into::into).collect();

        host_msgs
    }
}

impl coordinator_bindings::paxos::default::logger::Host for ComponentRunStates {
    // Delegate the log calls to our stored HostLogger.
    async fn log_debug(&mut self, msg: String) {
        self.logger.log_debug(msg);
    }

    async fn log_info(&mut self, msg: String) {
        self.logger.log_info(msg);
    }

    async fn log_warn(&mut self, msg: String) {
        self.logger.log_warn(msg);
    }

    async fn log_error(&mut self, msg: String) {
        self.logger.log_error(msg);
    }
}
