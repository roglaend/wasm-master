use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use wasmtime::component::{Component, Linker, ResourceAny};
use wasmtime::{Config, Engine, ProfilingStrategy, Store};
use wasmtime_wasi::{
    DirPerms, FilePerms, IoView, ResourceTable, WasiCtx, WasiCtxBuilder, WasiView,
};

use crate::bindings;
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
    pub fn new(node: Node) -> Self {
        let host_node = host_logger::HostNode {
            node_id: node.node_id,
            address: node.address.clone(),
            role: node.role as u64,
        };

        let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("Must have a parent")
            .parent()
            .expect("Workspace folder")
            .to_owned();

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
            logger: Arc::new(HostLogger::new_from_workspace(host_node)),
        }
    }
}

pub struct PaxosWasmtime {
    pub _engine: Engine,
    pub store: Mutex<Store<ComponentRunStates>>,
    pub bindings: bindings::PaxosWsWorld,
    pub resource_handle: ResourceAny,
}

impl PaxosWasmtime {
    pub async fn new(
        node: bindings::paxos::default::paxos_types::Node,
        nodes: Vec<bindings::paxos::default::paxos_types::Node>,
        is_leader: bool,
        run_config: RunConfig,
    ) -> Result<Self, Box<dyn Error>> {
        let mut config = Config::new();
        config.async_support(true);
        config.profiler(ProfilingStrategy::VTune);
        let engine = Engine::new(&config)?;

        let state = ComponentRunStates::new(node.clone());
        let mut store = Store::new(&engine, state);
        let mut linker = Linker::<ComponentRunStates>::new(&engine);

        wasmtime_wasi::add_to_linker_async(&mut linker)?;

        bindings::paxos::default::logger::add_to_linker(&mut linker, |s| s)?;

        let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("Must have a parent")
            .parent()
            .expect("Workspace folder")
            .to_owned();

        //    let composed_component = Component::from_file(
        //         &engine,
        //         workspace_dir.join("target/wasm32-wasip2/release/final_composed_udp_server.wasm"),
        //     )?;

        // TODO: Make the component/model swap much better, thankyou :)

        let composed_component = Component::from_file(
            &engine,
            workspace_dir.join("target/wasm32-wasip2/release/final_composed_tcp_server.wasm"),
        )?;

        let final_bindings =
            bindings::PaxosWsWorld::instantiate_async(&mut store, &composed_component, &linker)
                .await?;

        let guest = final_bindings.paxos_default_ws_server();
        let resource = guest.ws_server_resource();

        let resource_handle = resource
            .call_constructor(&mut store, &node, &nodes, is_leader, run_config)
            .await?;

        Ok(Self {
            _engine: engine,
            store: Mutex::new(store),
            bindings: final_bindings,
            resource_handle,
        })
    }

    //Helper methods to access WASM guest.
    pub fn guest<'a>(&'a self) -> &'a bindings::exports::paxos::default::ws_server::Guest {
        self.bindings.paxos_default_ws_server()
    }

    pub fn resource<'a>(
        &'a self,
    ) -> bindings::exports::paxos::default::ws_server::GuestWsServerResource<'a> {
        self.guest().ws_server_resource()
    }
}

impl bindings::paxos::default::logger::Host for ComponentRunStates {
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
