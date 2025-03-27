use crate::grpc_service::create_clients;
use crate::host_logger::HostLogger;
use crate::host_messenger::HostMessenger;
use crate::paxos_bindings;
use futures::future::{join, join_all};
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
    pub fn new(node_id: u64) -> Self {
        ComponentRunStates {
            wasi_ctx: WasiCtxBuilder::new()
                .inherit_stdio()
                .inherit_env()
                .inherit_args()
                .build(),
            resource_table: ResourceTable::new(),
            logger: Arc::new(HostLogger::new_from_workspace(node_id)),
        }
    }
}

pub struct PaxosWasmtime {
    pub _engine: Engine,
    pub store: tokio::sync::Mutex<Store<ComponentRunStates>>,
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

        let state = ComponentRunStates::new(node_id);
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
            workspace_dir.join("target/wasm32-wasip2/release/composed_paxos_coordinator.wasm"),
        )?;

        let final_bindings =
            paxos_bindings::PaxosWorld::instantiate_async(&mut store, &composed_component, &linker)
                .await?;
        let paxos_guest = final_bindings.paxos_default_paxos_coordinator();
        let paxos_resource = paxos_guest.paxos_coordinator_resource();
        let resource_handle = paxos_resource
            .call_constructor(&mut store, node_id, &nodes)
            .await?;

        Ok(Self {
            _engine: engine,
            store: tokio::sync::Mutex::new(store),
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
        "Hello".to_string()
    }

    
    // Wasm proposer for prepare/accept messages to be sent to acceptors.
    async fn send_message_forget(
        &mut self,
        nodes: Vec<paxos_bindings::paxos::default::network::Node>,
        message: paxos_bindings::paxos::default::network::NetworkMessage
    ) -> () {
        tokio::spawn(async move {
            let proto_msg: paxos_proto::NetworkMessage = message.clone().into();
            let endpoints: Vec<String> = nodes.into_iter().map(|node| node.address).collect();
            let clients_arc = create_clients(endpoints.clone()).await;
            let clients: Vec<_> = {
                let guard = clients_arc.lock();
                guard.unwrap().clone()
            };
            let send_futures = clients.into_iter().map(|mut client| {
                let req = tonic::Request::new(proto_msg.clone());
                async move { client.deliver_message(req).await }
            });
            join_all(send_futures).await;
        });
        ()    
    }   
    
    // For acceptors to send promise/accept messages back to proposer.
    async  fn send_message_to_forget(
        &mut self, 
        node: paxos_bindings::paxos::default::network::Node, 
        message: paxos_bindings::paxos::default::network::NetworkMessage
    ) -> () {
        todo!()
    }

    async fn send_message(
        &mut self,
        nodes: Vec<paxos_bindings::paxos::default::network::Node>,
        message: paxos_bindings::paxos::default::network::NetworkMessage,
    ) -> Vec<paxos_bindings::paxos::default::network::NetworkResponse> {
        self.logger.log_info(format!(
            "Sending network message of kind: {:?}",
            message.kind
        ));
        // Convert the WIT NetworkMessage into the proto-generated NetworkMessage.
        let proto_msg: paxos_proto::NetworkMessage = message.clone().into();

        // Extract endpoints from the nodes.
        let endpoints: Vec<String> = nodes.into_iter().map(|node| node.address).collect();

        // Delegate the sending to HostMessenger
        HostMessenger::send_message(endpoints, proto_msg, message.kind).await
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
