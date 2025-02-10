use anyhow::Context as _;
use std::sync::{Arc, RwLock};
use utils_wrpc_v2::handle_to_index;
use wit_bindgen_wrpc::{
    anyhow::Result,
    bytes::Bytes,
    futures::TryStreamExt as _,
    tokio,
    wrpc_transport::{self, ResourceBorrow, ResourceOwn, Serve},
};

mod bindings {
    wit_bindgen_wrpc::generate!({
        world: "proposer-world",
    });
}

use bindings::exports::paxos::wrpc::proposer::{
    Handler, HandlerProposerResource, ProposerResource, ProposerState,
};

#[derive(Clone, Default)]
struct MyProposer {
    proposer_resources: Arc<RwLock<Vec<MyProposerResource>>>,
}

impl<Ctx: Send> Handler<Ctx> for MyProposer {
    // type ProposerResource = MyProposerResource;
    // type ProposerResource = MyProposerResource
}

#[derive(Default)]
struct MyProposerResource {
    proposal_id: RwLock<u64>,
    last_value: RwLock<Option<String>>,
    num_nodes: RwLock<u64>,
}

// #[async_trait(?Send)]
impl<Ctx: Send> HandlerProposerResource<Ctx> for MyProposer {
    async fn new(&self, _: Ctx, num_nodes: u64) -> Result<ResourceOwn<ProposerResource>> {
        let new_resource = MyProposerResource {
            proposal_id: RwLock::new(0),
            last_value: RwLock::new(None),
            num_nodes: RwLock::new(num_nodes),
        };
        // let mut resource = self.resource.get_mut().unwrap();
        // self.resource = Arc::new(RwLock::new(resource));

        let mut proposer_resources = self.proposer_resources.write().unwrap();
        proposer_resources.push(new_resource);

        // Should always be same, since we want all consumers to get same resource
        let handle = proposer_resources.len().to_le_bytes();
        Ok(ResourceOwn::from(Bytes::copy_from_slice(&handle)))
    }

    async fn get_state(
        &self,
        _: Ctx,
        handle: ResourceBorrow<ProposerResource>,
    ) -> Result<ProposerState> {
        let index = handle_to_index(handle)?;

        let resources = self.proposer_resources.read().unwrap();
        let resource = resources
            .get(index)
            .context("invalid proposer resource handle")?;

        let result = Ok(ProposerState {
            proposal_id: *resource.proposal_id.read().unwrap(),
            last_value: resource.last_value.read().unwrap().clone(),
            num_acceptors: *resource.num_nodes.read().unwrap(),
        });
        result
    }

    async fn propose(
        &self,
        _cx: Ctx,
        handle: ResourceBorrow<ProposerResource>,
        proposal_id: u64,
        value: String,
    ) -> anyhow::Result<bool> {
        let index = handle_to_index(handle)?;

        let resources = self.proposer_resources.read().unwrap();
        let resource = resources
            .get(index)
            .context("invalid proposer resource handle")?;

        // Validate the proposal: only accept a new proposal if its ID is larger than the current.
        let mut current_id = resource.proposal_id.write().unwrap();
        if proposal_id <= *current_id {
            log::warn!(
                "Proposer: Rejected proposal {} (<= current {})",
                proposal_id,
                *current_id
            );
            return Ok(false);
        }
        *current_id = proposal_id;

        // Update the last proposed value.
        *resource.last_value.write().unwrap() = Some(value.clone());
        log::info!(
            "Proposer: Proposing value '{}' with ID {}",
            value,
            proposal_id
        );

        // NOTE:
        // The actual communication with acceptors (the prepare/accept phases) is now handled
        // by the higher-level Paxos WASM component. This function only updates the local state.
        Ok(true)
    }
}

// This is the export-serving function. It wires up the Paxos component with the transport.
async fn serve_exports(wrpc: &impl Serve) {
    // The generated bindings use the `serve` function to wire up the exports from our component.
    let invocations = bindings::serve(wrpc, MyProposer::default())
        .await
        .expect("failed to serve Paxos exports");

    // Spawn a task for each incoming invocation stream.
    for (instance, name, stream) in invocations {
        tokio::spawn(async move {
            eprintln!("Serving instance {instance} export {name}");
            if let Err(e) = stream.try_collect::<Vec<_>>().await {
                eprintln!("Error processing {instance} {name}: {e:?}");
            }
        });
    }
}

// // 3) Export the Proposer
// export!(MyHandlerProposer);

// fn main(){
//     let wrpc = wrpc::Serve
//     crate::serve_exports(&wrpc)
// }
