use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use wasmtime::component::{Component, Linker, Resource};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{
    DirPerms, FilePerms, IoView, ResourceTable, WasiCtx, WasiCtxBuilder, WasiView,
};

use crate::bindings::paxos::default::network_types::NetworkMessage;
use crate::bindings::paxos::default::{network_client, network_server};
use crate::host_network_client::NativeTcpClient;
use crate::host_network_server::NativeTcpServer;

use crate::bindings;
use crate::bindings::paxos::default::logger::{self, Level};
use crate::bindings::paxos::default::paxos_types::{Node, RunConfig};
use crate::host_logger::{self, HostLogger};

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
    pub fn new(node: Node, log_level: Level) -> Self {
        let host_node = host_logger::HostNode {
            node_id: node.node_id,
            address: node.address.clone(),
            role: node.role,
        };

        let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("Must have a parent")
            .parent()
            .expect("Workspace folder")
            .to_owned();

        let state_host_path = workspace_dir.join("paxos-wasm/logs/state");
        fs::create_dir_all(&state_host_path)
            .expect(&format!("Failed to create state dir {:?}", state_host_path));

        let mut builder = WasiCtxBuilder::new();
        builder.inherit_stdio();
        builder.inherit_env();
        builder.inherit_args();
        builder.inherit_network();
        builder
            .preopened_dir(
                workspace_dir.join("paxos-wasm/logs/state"),
                "/state",
                DirPerms::all(),
                FilePerms::all(),
            )
            .expect("Failed to preopen dir");

        let wasi_ctx = builder.build();

        ComponentRunStates {
            wasi_ctx,
            resource_table: ResourceTable::new(),

            logger: Arc::new(HostLogger::new_from_workspace(host_node, log_level)),
        }
    }
}

pub struct NetworkServerResource {
    pub server: NativeTcpServer,
}

pub struct NetworkClientResource {
    pub client: NativeTcpClient,
}

pub struct PaxosWasmtime {
    pub store: Mutex<Store<ComponentRunStates>>,
    pub bindings: bindings::PaxosRunnerWorld,
}

impl PaxosWasmtime {
    pub async fn new(
        engine: &Engine,
        node: bindings::paxos::default::paxos_types::Node,
        log_level: Level,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let state = ComponentRunStates::new(node.clone(), log_level);
        let mut store = Store::new(&engine, state);
        let mut linker = Linker::<ComponentRunStates>::new(&engine);

        wasmtime_wasi::add_to_linker_async(&mut linker)?;

        bindings::paxos::default::logger::add_to_linker(&mut linker, |s| s)?;
        bindings::paxos::default::network_server::add_to_linker(&mut linker, |s| s)?;
        bindings::paxos::default::network_client::add_to_linker(&mut linker, |s| s)?;

        let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("Must have a parent")
            .parent()
            .expect("Workspace folder")
            .to_owned();

        let composed_component = Component::from_file(
            &engine,
            workspace_dir.join("target/wasm32-wasip2/release/final_composed_runner.wasm"),
        )?;

        let final_bindings =
            bindings::PaxosRunnerWorld::instantiate_async(&mut store, &composed_component, &linker)
                .await?;

        Ok(Self {
            store: Mutex::new(store),
            bindings: final_bindings,
        })
    }

    pub async fn run(
        &mut self,
        node: bindings::paxos::default::paxos_types::Node,
        nodes: Vec<bindings::paxos::default::paxos_types::Node>,
        is_leader: bool,
        run_config: RunConfig,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut store = self.store.lock().await;
        let guest = self.bindings.paxos_default_runner();
        let resource = guest.runner_resource();

        let resource_handle = resource
            .call_constructor(&mut *store, &node, &nodes, is_leader, run_config)
            .await?;

        // Call run
        resource.call_run(&mut *store, resource_handle).await?;

        Ok(())
    }
}

impl network_server::Host for ComponentRunStates {}

impl network_server::HostNetworkServerResource for ComponentRunStates {
    async fn new(&mut self) -> Resource<NetworkServerResource> {
        let server = self.resource_table.push(NetworkServerResource {
            server: NativeTcpServer::new(),
        });
        server.unwrap()
    }

    async fn setup_listener(
        &mut self,
        resource: Resource<NetworkServerResource>,
        bind_addr: String,
    ) {
        let server = self.resource_table.get_mut(&resource).unwrap();
        server.server.setup_listener(&bind_addr);
    }

    async fn get_messages(
        &mut self,
        resource: Resource<NetworkServerResource>,
        max: u64,
    ) -> Vec<NetworkMessage> {
        let server = self.resource_table.get_mut(&resource).unwrap();
        server.server.get_messages(max)
    }

    async fn get_message(
        &mut self,
        self_: wasmtime::component::Resource<NetworkServerResource>,
    ) -> Option<NetworkMessage> {
        let server = self.resource_table.get_mut(&self_).unwrap();
        server.server.get_message()
    }

    async fn drop(
        &mut self,
        rep: wasmtime::component::Resource<NetworkServerResource>,
    ) -> wasmtime::Result<()> {
        let _ = self.resource_table.delete(rep)?;
        Ok(())
    }
}

impl network_client::HostNetworkClientResource for ComponentRunStates {
    async fn new(&mut self) -> Resource<NetworkClientResource> {
        let client = self.resource_table.push(NetworkClientResource {
            client: NativeTcpClient::new(),
        });
        client.unwrap()
    }

    async fn send_message(
        &mut self,
        resource: Resource<NetworkClientResource>,
        node: Vec<Node>,
        message: network_client::NetworkMessage,
    ) -> Vec<NetworkMessage> {
        let client = self.resource_table.get_mut(&resource).unwrap();
        client.client.send_message(node, message)
    }

    async fn send_message_forget(
        &mut self,
        resource: Resource<NetworkClientResource>,
        node: Vec<Node>,
        message: network_client::NetworkMessage,
    ) {
        let client = self.resource_table.get_mut(&resource).unwrap();
        client.client.send_message_forget(node, message)
    }

    async fn drop(&mut self, rep: Resource<NetworkClientResource>) -> wasmtime::Result<()> {
        let _ = self.resource_table.delete(rep)?;
        Ok(())
    }
}

impl network_client::Host for ComponentRunStates {}

impl logger::Host for ComponentRunStates {
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
