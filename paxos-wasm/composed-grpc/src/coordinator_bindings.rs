// TODO: Create a shared package for this
wasmtime::component::bindgen! {{
    path: "../shared/wit",
    world: "paxos-world",
    additional_derives: [Clone],
    async: true,
    // TODO: Try async again later
}}

use std::error::Error;

use crate::coordinator_bindings;
use crate::traits::{ComponentRunStates, PaxosBindings};
use futures::future::BoxFuture;
use wasmtime::Store;
use wasmtime::component::{Component, Linker, ResourceAny};

impl PaxosBindings for coordinator_bindings::PaxosWorld {
    type Node = coordinator_bindings::paxos::default::paxos_types::Node;
    type RunConfig = coordinator_bindings::paxos::default::paxos_types::RunConfig;
    type NetworkMessage = coordinator_bindings::paxos::default::network_types::NetworkMessage;
    type Guest = coordinator_bindings::exports::paxos::default::paxos_coordinator::Guest;
    // We parameterize the guest resource over the lifetime.
    type GuestResource<'a> = coordinator_bindings::exports::paxos::default::paxos_coordinator::GuestPaxosCoordinatorResource<'a>;

    fn instantiate_async<'a>(
        store: &'a mut Store<ComponentRunStates>,
        component: &'a Component,
        linker: &'a Linker<ComponentRunStates>,
    ) -> BoxFuture<'a, Result<Self, Box<dyn Error>>>
    where
        Self: Sized,
    {
        // Propagate the lifetime 'a.
        Box::pin(async move {
            let bindings =
                coordinator_bindings::PaxosWorld::instantiate_async(store, component, linker)
                    .await?;
            Ok(bindings)
        })
    }

    fn guest(&self) -> &Self::Guest {
        // We assume paxos_default_paxos_coordinator() returns a reference.
        self.paxos_default_paxos_coordinator()
    }

    fn guest_resource<'a>(&'a self) -> Self::GuestResource<'a> {
        // Here, paxos_coordinator_resource() returns the guest resource by value
        // (with its lifetime tied to self).
        self.paxos_default_paxos_coordinator()
            .paxos_coordinator_resource()
    }

    fn construct_resource<'a>(
        &'a self,
        store: &'a mut Store<ComponentRunStates>,
        node: &Self::Node,
        nodes: &[Self::Node],
        is_leader: bool,
        run_config: &Self::RunConfig,
    ) -> BoxFuture<'a, Result<ResourceAny, Box<dyn Error>>> {
        Box::pin(async move {
            // Capture the guest resource into a local binding explicitly typed.
            let guest_resource: Self::GuestResource<'a> = self.guest_resource();
            // Call the constructor.
            let resource_handle = guest_resource
                .call_constructor(store, node, nodes, is_leader, run_config)
                .await?;
            Ok(resource_handle)
        })
    }
}
