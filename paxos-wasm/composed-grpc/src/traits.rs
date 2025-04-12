use std::error::Error;
use std::sync::Arc;

use futures::future::BoxFuture;
use paxos_wasm_bindings_types::paxos::paxos_types::Node;
use wasmtime_wasi::{IoView, ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

use crate::host_logger::HostLogger;

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
        ComponentRunStates {
            wasi_ctx: WasiCtxBuilder::new()
                .inherit_stdio()
                .inherit_env()
                .inherit_args()
                .build(),
            resource_table: ResourceTable::new(),
            logger: Arc::new(HostLogger::new_from_workspace(node)),
        }
    }
}

use wasmtime::Store;
use wasmtime::component::{Component, Linker, ResourceAny};

pub trait PaxosBindings {
    /// The type for a Paxos node.
    type Node: Clone;
    /// The type for the run configuration.
    type RunConfig;
    /// The type for network messages.
    type NetworkMessage;
    /// The type representing the guest interface.
    type Guest;
    /// The type representing the guest resource,
    /// parameterized by a lifetime.
    type GuestResource<'a>
    where
        Self: 'a;

    /// Asynchronously instantiate the bindings.
    fn instantiate_async<'a>(
        store: &'a mut Store<ComponentRunStates>,
        component: &'a Component,
        linker: &'a Linker<ComponentRunStates>,
    ) -> BoxFuture<'a, Result<Self, Box<dyn Error>>>
    where
        Self: Sized;

    /// Access the guest interface.
    fn guest(&self) -> &Self::Guest;
    /// Return the guest resource with the lifetime of the borrow.
    fn guest_resource<'a>(&'a self) -> Self::GuestResource<'a>;

    /// Construct the resource handle needed to drive the guest.
    ///
    /// This method hides differences in the constructor arguments for different resources.
    /// The returned future should yield a wasmtime::component::ResourceAny.
    fn construct_resource<'a>(
        &'a self,
        store: &'a mut Store<ComponentRunStates>,
        node: &Self::Node,
        nodes: &[Self::Node],
        is_leader: bool,
        run_config: &Self::RunConfig,
    ) -> BoxFuture<'a, Result<ResourceAny, Box<dyn Error>>>;
}
