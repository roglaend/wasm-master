use std::error::Error;
use tokio::sync::Mutex;
use wasmtime::{Engine, Store};
use wasmtime_wasi::ResourceTable;
use wasmtime_wasi::p2::{IoView, WasiCtx, WasiCtxBuilder, WasiView};

use crate::bindings::paxos::default::paxos_types::Value;
use crate::bindings::{self, PaxosClientWorldPre};
// use crate::host_logger::{self, HostLogger};

pub type PaxosError = Box<dyn Error + Send + Sync + 'static>;

pub struct ComponentRunStates {
    // These two are required basically as a standard way to enable the impl of WasiView
    pub wasi_ctx: WasiCtx,
    pub resource_table: ResourceTable,
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
    pub fn new() -> Self {
        ComponentRunStates {
            wasi_ctx: WasiCtxBuilder::new()
                .inherit_stdio()
                .inherit_env()
                .inherit_args()
                .inherit_network()
                .build(),
            resource_table: ResourceTable::new(),
        }
    }
}

pub struct PaxosWasmtime {
    pub store: Mutex<Store<ComponentRunStates>>,
    pub bindings: bindings::PaxosClientWorld,
}

impl PaxosWasmtime {
    pub async fn new(
        engine: &Engine,
        paxos_client_pre: PaxosClientWorldPre<ComponentRunStates>,
    ) -> Result<Self, PaxosError> {
        let state = ComponentRunStates::new();
        let mut store = Store::new(engine, state);

        let bindings =
            bindings::PaxosClientWorldPre::instantiate_async(&paxos_client_pre, &mut store).await?;

        Ok(Self {
            store: Mutex::new(store),
            bindings,
        })
    }

    pub async fn perform_request(
        &self,
        leader_address: String,
        value: Value,
    ) -> Result<Option<bindings::exports::paxos::default::paxos_client::ClientResponse>, PaxosError>
    {
        let mut store = self.store.lock().await;
        let result = self
            .bindings
            .paxos_default_paxos_client()
            .call_perform_request(&mut *store, &leader_address, &value)
            .await?;
        Ok(result)
    }
}
