use anyhow::Context as _;
use std::{
    any,
    sync::{Arc, RwLock},
};
use tokio::task::JoinSet;
use utils_wrpc_v2::{handle_to_index, run_component_wrpc_exports};
use wit_bindgen_wrpc::{
    anyhow::Result,
    bytes::Bytes,
    tracing::{info, warn},
    wrpc_transport::{tcp::Client, ResourceBorrow, ResourceOwn},
};

mod bindings {
    wit_bindgen_wrpc::generate!( {
        world: "proposer-world",
    });
}

use bindings::exports::paxos::wrpc::proposer::{
    Handler, HandlerProposerResource, ProposerResource, ProposerState,
};

#[derive(Clone, Default)]
struct MyProposer {
    // Global storage for proposer resources, protected by an RwLock.
    proposer_resources: Arc<RwLock<Vec<MyProposerResource>>>,

    test_value: Arc<RwLock<String>>,
}

// Implement the top-level export trait.
impl<Ctx: Send> Handler<Ctx> for MyProposer {
    async fn get_test_value(&self, _ctx: Ctx) -> anyhow::Result<String> {
        let value = self.test_value.read().unwrap().to_string();
        // Ok("ghghjghjghjgjh".to_string())
        Ok(value.to_string())
    }

    // Wanted this to return ResourceBorrow instead of ResourceOwn due to wanting it shared, but not allowed to.
    async fn get_shared_resource(
        &self,
        _ctx: Ctx,
    ) -> anyhow::Result<ResourceOwn<ProposerResource>> {
        // Check if we have at least one shared resource.
        let resources = self.proposer_resources.read().unwrap();
        if resources.is_empty() {
            anyhow::bail!("No shared resource available");
        }

        // We hardcode the resource to be at index 0.
        // The handle is the little-endian encoding of 0 (the index).
        let handle_bytes = 0usize.to_le_bytes();
        // Construct the ResourceBorrow from the handle bytes.
        Ok(ResourceOwn::from(Bytes::copy_from_slice(&handle_bytes)))
    }
}

// Each resource holds its mutable state behind RwLocks.
#[derive(Default)]
struct MyProposerResource {
    proposal_id: RwLock<u64>,
    last_value: RwLock<Option<String>>,
    self_address: RwLock<String>,
    acceptor_addresses: RwLock<Vec<String>>,
}

impl<Ctx: Send> HandlerProposerResource<Ctx> for MyProposer {
    async fn new(
        &self,
        _ctx: Ctx,
        self_address: String,
        acceptor_addresses: Vec<String>,
    ) -> Result<ResourceOwn<ProposerResource>> {
        {
            // Acquire a write lock and update the test_value.
            let mut guard = self.test_value.write().unwrap();
            *guard = "test_value_set_in_constructor".to_string();
        } // The lock is dropped here.

        // Create a new resource with its fields wrapped in RwLocks.
        let resource = MyProposerResource {
            proposal_id: RwLock::new(0),
            last_value: RwLock::new(None),
            self_address: RwLock::new(self_address),
            acceptor_addresses: RwLock::new(acceptor_addresses),
        };

        // Push the new resource into our global vector.
        let mut resources = self.proposer_resources.write().unwrap();
        resources.push(resource);

        // Use the vector length (in little-endian) as the resource handle.
        // let handle = resources.len().to_le_bytes();

        // Always return the same handle. In our case, we use the index 0.
        let handle = 0usize.to_le_bytes();

        Ok(ResourceOwn::from(Bytes::copy_from_slice(&handle)))
    }

    async fn get_state(
        &self,
        _ctx: Ctx,
        handle: ResourceBorrow<ProposerResource>,
    ) -> Result<ProposerState> {
        let index = handle_to_index(handle)?;
        let resources = self.proposer_resources.read().unwrap();
        let resource = resources
            .get(index)
            .context("invalid proposer resource handle")?;

        // Read all fields using read locks.
        let proposal_id = *resource.proposal_id.read().unwrap();
        let last_value = resource.last_value.read().unwrap().clone();
        let self_address = resource.self_address.read().unwrap().clone();
        let num_acceptors = resource.acceptor_addresses.read().unwrap().len() as u64;
        Ok(ProposerState {
            proposal_id,
            last_value,
            self_address,
            num_acceptors,
        })
    }

    async fn propose(
        &self,
        _ctx: Ctx,
        handle: ResourceBorrow<ProposerResource>,
        proposal_id: u64,
        value: String,
    ) -> anyhow::Result<bool> {
        let index = handle_to_index(handle)?;
        let resources = self.proposer_resources.read().unwrap();
        let resource = resources
            .get(index)
            .context("invalid proposer resource handle")?;

        let acceptor_addresses = resource.acceptor_addresses.read().unwrap();
        let total_acceptors = acceptor_addresses.len();
        let majority = (total_acceptors / 2) + 1;

        // --- Phase 1: PREPARE ---
        info!("Starting PREPARE phase for proposal {}", proposal_id);
        let mut prepare_tasks = JoinSet::new();
        // Need to call exported handler function on all acceptors over wrpc
        for addr in acceptor_addresses.iter().cloned() {
            let pid = proposal_id;
            let acceptor_resource_handle = self.acceptor_resource_handles.get(addr);
            prepare_tasks.spawn(async move {
                let wrpc = Client::from(addr.clone());
                let result = Acceptor::AcceptorResource::prepare(
                    &wrpc,
                    (),
                    &acceptor_resource_handle,
                    pid,
                );
            });
        }

        let mut prepare_ok = 0;
        let mut responded = 0;
        while let Some(res) = prepare_tasks.join_next().await {
            responded += 1;
            match res {
                Ok(Ok(true)) => prepare_ok += 1,
                Ok(Ok(false)) => warn!("An acceptor rejected prepare"),
                Ok(Err(err)) => warn!("Error during prepare: {:?}", err),
                Err(err) => warn!("Task join error: {:?}", err),
            }
        }
        info!(
            "PREPARE phase: {} acceptors responded positively out of {}",
            prepare_ok, responded
        );
        if prepare_ok < majority {
            warn!("PREPARE phase failed to achieve majority");
            return Ok(false);
        }
        Ok(true);

        // Write-lock the proposal ID field so we can compare and update.
        {
            let mut current_id = resource.proposal_id.write().unwrap();
            if proposal_id <= *current_id {
                warn!(
                    "Proposer: Rejected proposal {} (<= current {})",
                    proposal_id, *current_id
                );
                return Ok(false);
            }
            *current_id = proposal_id;
        }
        // Update the last proposed value.
        {
            let mut last_value = resource.last_value.write().unwrap();
            *last_value = Some(value.clone());
        }
        info!(
            "Proposer: Proposing value '{}' with ID {}",
            value, proposal_id
        );
        Ok(true)
    }
}

/// This function wires up our component's exports to the wRPC transport. Similar to wit_bindgen_wrpc documentation.
// async fn serve_exports(wrpc: &impl Serve) {
//     let invocations = bindings::serve(wrpc, MyProposer::default())
//         .await
//         .expect("failed to serve proposer exports");

// invocations
//     .into_iter()
//     .for_each(|(instance, name, stream)| {
//         tokio::spawn(async move {
//             eprintln!("serving {instance} {name}");
//             stream.try_collect::<Vec<_>>().await.unwrap();
//         });
//     });
// }

/// A thin wrapper that uses the generic function for the proposer.
pub async fn run_proposer_wrpc_exports(addr: &str) -> anyhow::Result<()> {
    run_component_wrpc_exports(addr, MyProposer::default(), |srv, handler| async move {
        // The bindings’ serve function wires up the exports.
        bindings::serve(srv.as_ref(), handler).await
    })
    .await
}

// Run the proposer component and keep serving until a shutdown signal is received.
// pub async fn run_proposer_wrpc_exports(addr: &str) -> anyhow::Result<()> {
//     // Initialize logging.
//     tracing_subscriber::fmt::init();

//     // Bind a TCP listener on the specified address.
//     let lis = TcpListener::bind(&addr)
//         .await
//         .with_context(|| format!("failed to bind TCP listener on `{addr}`"))?;

//     // Create the wRPC server instance.
//     let srv = Arc::new(wrpc_transport::Server::default());

//     // Spawn a task to continuously accept incoming TCP connections.
//     let accept_handle = {
//         let srv = Arc::clone(&srv);
//         tokio::spawn(async move {
//             loop {
//                 if let Err(err) = srv.accept(&lis).await {
//                     error!(?err, "failed to accept TCP connection");
//                 }
//             }
//         })
//     };

//     info!("Proposer component is now serving exports on {addr}");

//     // Use the generated bindings to serve exports.
//     let invocations = bindings::serve(srv.as_ref(), MyProposer::default())
//         .await
//         .context("failed to serve proposer exports")?;

//     // Conflate all invocation streams into one unified stream.
//     let mut invocations: SelectAll<_> = invocations
//         .into_iter()
//         .map(|(instance, name, stream)| {
//             info!("serving {instance} {name}");
//             stream.map(move |res| (instance, name, res))
//         })
//         .collect();

//     // Prepare a join set to handle spawned tasks.
//     let mut tasks = JoinSet::new();
//     // Prepare the shutdown signal.
//     let shutdown = signal::ctrl_c();
//     pin!(shutdown);

//     loop {
//         select! {
//             // Process incoming invocations.
//             Some((instance, name, res)) = invocations.next() => {
//                 match res {
//                     Ok(fut) => {
//                         debug!(?instance, ?name, "invocation accepted");
//                         tasks.spawn(async move {
//                             if let Err(err) = fut.await {
//                                 warn!(?err, "failed to handle invocation");
//                             } else {
//                                 info!(?instance, ?name, "invocation successfully handled");
//                             }
//                         });
//                     },
//                     Err(err) => {
//                         warn!(?err, ?instance, ?name, "failed to accept invocation");
//                     },
//                 }
//             },
//             // Process completed tasks.
//             Some(task_res) = tasks.join_next() => {
//                 if let Err(err) = task_res {
//                     error!(?err, "failed to join task");
//                 }
//             },
//             // Shutdown signal received.
//             _ = &mut shutdown => {
//                 info!("Shutdown signal received. Aborting accept loop and waiting for tasks...");
//                 accept_handle.abort();
//                 // Wait for all spawned tasks to complete.
//                 while let Some(task_res) = tasks.join_next().await {
//                     if let Err(err) = task_res {
//                         error!(?err, "failed to join task");
//                     }
//                 }
//                 break;
//             }
//         }
//     }

//     Ok(())
// }
