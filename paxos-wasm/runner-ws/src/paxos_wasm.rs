use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use wasmtime::component::{Component, Linker, Resource};
use wasmtime::{Engine, Store};
use wasmtime_wasi::p2::{IoView, WasiCtx, WasiCtxBuilder, WasiView};
use wasmtime_wasi::{DirPerms, FilePerms, ResourceTable};

use crate::bindings::paxos::default::host_control::HostCmd;
use crate::bindings::paxos::default::network_types::NetworkMessage;
use crate::bindings::paxos::default::{host_control, network_client, network_server};
use crate::host_control::HostControlHandler;
use crate::host_network_client::NativeTcpClient;
use crate::host_network_server::NativeTcpServer;

use crate::bindings;
use crate::bindings::paxos::default::logger::{self, Level};
use crate::bindings::paxos::default::paxos_types::{Node, RunConfig};
use crate::bindings::paxos::default::storage;
use crate::host_logger::{self, HostLogger};
use crate::host_storage::{HostStorage, StorageRequest};

pub struct ComponentRunStates {
    // These two are required basically as a standard way to enable the impl of WasiView
    pub wasi_ctx: WasiCtx,
    pub resource_table: ResourceTable,

    pub logger: Arc<HostLogger>,
    pub control: Arc<HostControlHandler>,
    pub state_host_path: PathBuf,
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
    /// New constructor that takes an optional logs directory (where logs and state will live).
    pub fn new(node: &Node, log_level: Level, logs_dir: Option<&Path>) -> Self {
        let host_node = host_logger::HostNode {
            node_id: node.node_id,
            address: node.address.clone(),
            role: node.role,
        };

        // If no custom logs_dir, default to workspace logs
        let logs_dir: PathBuf = if let Some(dir) = logs_dir {
            dir.to_path_buf()
        } else {
            let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("Must have a parent")
                .parent()
                .expect("Workspace folder")
                .to_owned();
            workspace_dir.join("logs")
        };

        // Create logs dir if needed
        std::fs::create_dir_all(&logs_dir).expect("Failed to create logs dir");

        // Put the state dir inside the logs dir
        let state_host_path = logs_dir.join("state");
        std::fs::create_dir_all(&state_host_path)
            .expect(&format!("Failed to create state dir {:?}", state_host_path));

        // Setup Wasi preopened dir for state
        let mut builder = WasiCtxBuilder::new();
        builder.inherit_stdio();
        builder.inherit_env();
        builder.inherit_args();
        builder.inherit_network();
        builder
            .preopened_dir(
                &state_host_path,
                "/state",
                DirPerms::all(),
                FilePerms::all(),
            )
            .expect("Failed to preopen dir");

        let wasi_ctx = builder.build();

        let logger = Arc::new(HostLogger::new_with_logs_dir(
            host_node, &logs_dir, log_level,
        ));

        let control = Arc::new(HostControlHandler::new());

        ComponentRunStates {
            wasi_ctx,
            resource_table: ResourceTable::new(),
            logger,
            control,
            state_host_path,
        }
    }
}

pub struct NetworkServerResource {
    pub server: NativeTcpServer,
}

pub struct NetworkClientResource {
    pub client: NativeTcpClient,
}

pub struct StorageResource {
    pub key: String,
    pub max_snapshots: u64,
    pub storage: Arc<HostStorage>,
}

pub struct PaxosWasmtime {
    pub store: Mutex<Store<ComponentRunStates>>,
    pub bindings: bindings::PaxosRunnerWorld,

    pub control_handle: Arc<HostControlHandler>,
}

impl PaxosWasmtime {
    pub async fn new(
        engine: &Engine,
        node: &Node,
        log_level: Level,
        wasm_path: String,
        logs_dir: Option<&Path>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let _ = logs_dir;
        let state = ComponentRunStates::new(node, log_level, logs_dir);
        let control_handle = state.control.clone();
        let mut store = Store::new(&engine, state);
        let mut linker = Linker::<ComponentRunStates>::new(&engine);

        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;

        bindings::paxos::default::logger::add_to_linker(&mut linker, |s| s)?;
        bindings::paxos::default::host_control::add_to_linker(&mut linker, |s| s)?;

        bindings::paxos::default::network_server::add_to_linker(&mut linker, |s| s)?;
        bindings::paxos::default::network_client::add_to_linker(&mut linker, |s| s)?;
        bindings::paxos::default::storage::add_to_linker(&mut linker, |s| s)?;

        let composed_component = Component::from_file(&engine, PathBuf::from(&wasm_path))?;

        let final_bindings =
            bindings::PaxosRunnerWorld::instantiate_async(&mut store, &composed_component, &linker)
                .await?;

        Ok(Self {
            store: Mutex::new(store),
            bindings: final_bindings,
            control_handle,
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

impl host_control::Host for ComponentRunStates {
    // Delegate to our embedded HostControlHandler
    async fn ping(&mut self) {
        self.control.record_ping().await
    }

    async fn get_command(&mut self) -> Option<HostCmd> {
        self.control.get_command().await
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

impl network_client::Host for ComponentRunStates {}

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

impl storage::Host for ComponentRunStates {}

impl storage::HostStorageResource for ComponentRunStates {
    async fn new(
        &mut self,
        key: String,
        max_snapshots: u64,
    ) -> wasmtime::component::Resource<StorageResource> {
        println!("State from host is being used");
        let state_host_path = &self.state_host_path;
        let res = StorageResource {
            key,
            max_snapshots,
            storage: HostStorage::new(state_host_path.clone()),
        };
        self.resource_table.push(res).unwrap()
    }

    async fn save_state(
        &mut self,
        this: wasmtime::component::Resource<StorageResource>,
        state: Vec<u8>,
    ) -> Result<(), String> {
        let res = self.resource_table.get(&this).unwrap();
        res.storage.enqueue(StorageRequest::SaveState {
            key: res.key.clone(),
            data: state,
        });
        Ok(())
    }

    async fn load_state(
        &mut self,
        this: wasmtime::component::Resource<StorageResource>,
    ) -> Result<Vec<u8>, String> {
        let res = self.resource_table.get(&this).unwrap();
        println!("Loading state from host");
        res.storage.load_current_state(&res.key).await
    }

    async fn flush_state(
        &mut self,
        _this: wasmtime::component::Resource<StorageResource>,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn save_change(
        &mut self,
        this: wasmtime::component::Resource<StorageResource>,
        change: Vec<u8>,
    ) -> Result<(), String> {
        let res = self.resource_table.get(&this).unwrap();
        res.storage.enqueue(StorageRequest::SaveChange {
            key: res.key.clone(),
            data: change,
        });
        Ok(())
    }

    async fn flush_changes(
        &mut self,
        _this: wasmtime::component::Resource<StorageResource>,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn checkpoint(
        &mut self,
        this: wasmtime::component::Resource<StorageResource>,
        state: Vec<u8>,
        timestamp: String,
    ) -> Result<(), String> {
        let res = self.resource_table.get(&this).unwrap();
        res.storage.enqueue(StorageRequest::Checkpoint {
            key: res.key.clone(),
            data: state,
            timestamp,
            max_snapshots: res.max_snapshots,
        });
        Ok(())
    }

    async fn list_snapshots(
        &mut self,
        this: wasmtime::component::Resource<StorageResource>,
    ) -> Vec<String> {
        let res = self.resource_table.get(&this).unwrap();
        let res = res.storage.list_snapshots(&res.key).await;
        res
    }

    async fn prune_snapshots(
        &mut self,
        this: wasmtime::component::Resource<StorageResource>,
        retain: u64,
    ) -> Result<(), String> {
        let res = self.resource_table.get(&this).unwrap();
        res.storage.prune_snapshots(&res.key, retain).await
    }

    async fn load_state_and_changes(
        &mut self,
        this: wasmtime::component::Resource<StorageResource>,
        num_snapshots: u64,
    ) -> Result<(Vec<Vec<u8>>, Vec<Vec<u8>>), String> {
        let res = self.resource_table.get(&this).unwrap();
        res.storage
            .load_state_and_changes(&res.key, num_snapshots)
            .await
    }

    async fn delete(&mut self, this: wasmtime::component::Resource<StorageResource>) -> bool {
        let res = self.resource_table.get(&this).unwrap();
        res.storage.delete(&res.key)
    }

    async fn drop(
        &mut self,
        this: wasmtime::component::Resource<StorageResource>,
    ) -> wasmtime::Result<()> {
        self.resource_table.delete(this)?;
        Ok(())
    }
}
