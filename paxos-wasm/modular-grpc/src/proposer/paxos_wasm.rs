use crate::proposer::host_logger::{self, HostLogger};
use crate::proposer::host_messenger::HostMessenger;
use crate::proposer::paxos_bindings::paxos::default::paxos_types::{Node, RunConfig};
// use crate::paxos_bindings::paxos::default::proposer_agent::RunConfig;
use crate::proposer::paxos_bindings::{self, MessagePayloadExt};
use proto::paxos_proto;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use wasmtime::component::{Component, Linker, ResourceAny};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{IoView, ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

pub struct ComponentRunStates {
    // These two are required basically as a standard way to enable the impl of WasiView
    pub wasi_ctx: WasiCtx,
    pub resource_table: ResourceTable,
    pub logger: Arc<HostLogger>,
}

impl IoView for ComponentRunStates {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.resource_table
    }
}

impl WasiView for ComponentRunStates {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi_ctx
    }
}

impl ComponentRunStates {
    pub fn new(node: Node) -> Self {
        let host_node = host_logger::HostNode {
            node_id: node.node_id,
            address: node.address.clone(),
            role: node.role as u64,
        };
        ComponentRunStates {
            wasi_ctx: WasiCtxBuilder::new()
                .inherit_stdio()
                .inherit_env()
                .inherit_args()
                .build(),
            resource_table: ResourceTable::new(),

            logger: Arc::new(HostLogger::new_from_workspace(host_node)),
        }
    }
}

pub struct PaxosWasmtime {
    pub _engine: Engine,
    pub store: tokio::sync::Mutex<Store<ComponentRunStates>>,
    pub bindings: paxos_bindings::ProposerAgentWorld,
    pub resource_handle: ResourceAny,
}

impl PaxosWasmtime {
    pub async fn new(
        node: paxos_bindings::paxos::default::paxos_types::Node,
        nodes: Vec<paxos_bindings::paxos::default::paxos_types::Node>,
        is_leader: bool,
        run_config: RunConfig,
    ) -> Result<Self, Box<dyn Error>> {
        let mut config = wasmtime::Config::default();
        config.async_support(true);
        let engine = Engine::new(&config)?;

        let state = ComponentRunStates::new(node.clone());
        let mut store = Store::new(&engine, state);
        let mut linker = Linker::<ComponentRunStates>::new(&engine);

        wasmtime_wasi::add_to_linker_async(&mut linker)?;

        paxos_bindings::paxos::default::network::add_to_linker(&mut linker, |s| s)?;
        paxos_bindings::paxos::default::logger::add_to_linker(&mut linker, |s| s)?;

        let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("Must have a parent")
            .parent()
            .expect("Workspace folder")
            .to_owned();

        let composed_component = Component::from_file(
            &engine,
            workspace_dir.join("target/wasm32-wasip2/release/composed_proposer_agent.wasm"),
        )?;

        let final_bindings = paxos_bindings::ProposerAgentWorld::instantiate_async(
            &mut store,
            &composed_component,
            &linker,
        )
        .await?;

        let proposer_guest = final_bindings.paxos_default_proposer_agent();
        let proposer_resource = proposer_guest.proposer_agent_resource();

        let resource_handle = proposer_resource
            .call_constructor(&mut store, &node, &nodes, is_leader, run_config)
            .await?;

        Ok(Self {
            _engine: engine,
            store: tokio::sync::Mutex::new(store),
            bindings: final_bindings,
            resource_handle,
        })
    }

    //Helper methods to access WASM guest.
    pub fn guest<'a>(
        &'a self,
    ) -> &'a paxos_bindings::exports::paxos::default::proposer_agent::Guest {
        self.bindings.paxos_default_proposer_agent()
    }

    pub fn resource<'a>(
        &'a self,
    ) -> paxos_bindings::exports::paxos::default::proposer_agent::GuestProposerAgentResource<'a>
    {
        self.guest().proposer_agent_resource()
    }
}

impl paxos_bindings::paxos::default::network::Host for ComponentRunStates {
    async fn send_hello(&mut self) -> String {
        "Hello".to_string()
    }

    async fn send_message_forget(
        &mut self,
        nodes: Vec<paxos_bindings::paxos::default::network::Node>,
        message: paxos_bindings::paxos::default::network::NetworkMessage,
    ) -> () {
        self.logger.log_info(format!(
            "[Host Messenger] Sending network message, fire-and-forget , with payload type: {}",
            message.payload.payload_type()
        ));

        let proto_msg: paxos_proto::NetworkMessage = message.clone().into();
        let endpoints: Vec<String> = nodes.into_iter().map(|node| node.address).collect();

        HostMessenger::send_message_forget(endpoints, proto_msg).await
    }

    async fn send_message(
        &mut self,
        nodes: Vec<paxos_bindings::paxos::default::network::Node>,
        message: paxos_bindings::paxos::default::network::NetworkMessage,
    ) -> Vec<paxos_bindings::paxos::default::network::NetworkMessage> {
        self.logger.log_info(format!(
            "[Host Messenger] Sending network message, and waiting for responses, with payload type: {}",
            message.payload.payload_type()
        ));

        let proto_msg: paxos_proto::NetworkMessage = message.clone().into();
        let endpoints: Vec<String> = nodes.into_iter().map(|node| node.address).collect();

        HostMessenger::send_message(endpoints, proto_msg).await
    }
}

impl paxos_bindings::paxos::default::logger::Host for ComponentRunStates {
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
