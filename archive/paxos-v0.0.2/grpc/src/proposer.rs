use tonic::Request;
use wasmtime::component::ResourceAny;

use tokio::sync::Mutex;
use std::sync::Arc;

pub mod paxos {
    tonic::include_proto!("paxos");
}

// Import the generated client
use paxos::acceptor_client::AcceptorClient;
use paxos::{PrepareRequest, AcceptRequest};


use wasmtime::component::bindgen;
use wasmtime::{Engine, Result, Store};
use wasmtime::component::{Component, Linker};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView, ResourceTable};

use crate::exports::paxos::proposer::proposerinterface::Promise;

bindgen!({
    path: "../wit/proposer/world.wit",
    world: "proposerworld"
});

struct ComponentRunStates  {
    ctx: WasiCtx,
    table: ResourceTable,
}

impl WasiView for ComponentRunStates  {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }
}

impl ComponentRunStates  {
    fn new() -> Self {
        let mut wasi = WasiCtxBuilder::new();
        ComponentRunStates  {
            ctx: wasi.build(),
            table: ResourceTable::new(),
        }
    }
}

pub struct WasmProposer {
    store: Mutex<Store<ComponentRunStates>>,
    bindings: Proposerworld, 
    resource_handle: ResourceAny,
}

impl WasmProposer {
    pub fn new() -> Result<Self> {
        let engine = Engine::default();
        let component = Component::from_file(&engine, "../proposer/target/wasm32-wasip1/release/proposer.wasm")?;
        
        let mut linker = Linker::<ComponentRunStates>::new(&engine);
        wasmtime_wasi::add_to_linker_sync(&mut linker)?;

        let state = ComponentRunStates::new();
        let mut store = Store::new(&engine, state);

        let bindings = Proposerworld::instantiate(&mut store, &component, &linker)?;

        let guest = bindings.paxos_proposer_proposerinterface();
        let resource = guest.proposerresource();
        let resource_handle = resource.call_constructor(&mut store, 1, 3)?;

        Ok(Self {
            store: Mutex::new(store),
            bindings, 
            resource_handle,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proposer = Arc::new(WasmProposer::new()?);  

    let acceptor_addresses = vec![
        "http://[::1]:50051",
        "http://[::1]:50052",
        "http://[::1]:50053",
    ];
    
    let mut clients = Vec::new();
    for address in &acceptor_addresses {
        clients.push(AcceptorClient::connect(address.to_string()).await?);
    }

    // Lock store and use bindings safely
    let mut store = proposer.store.lock().await;
    let guest = proposer.bindings.paxos_proposer_proposerinterface();
    let resource = guest.proposerresource();

    let my_proposer = proposer.resource_handle;


    // resource.call_increasecrnd(&mut *store, my_proposer)?;
    // resource.call_requestvalue(&mut *store,  my_proposer, 987)?;
    
    // PREPARE 
    let mut promises = Vec::new();
    for client in &mut clients {  
        let prepare = resource.call_makeprepare(&mut *store, my_proposer)?;
        println!("Sending prepare: {:?}", prepare);
        let prepare_request = Request::new(PrepareRequest {
            from: prepare.from,
            crnd: prepare.crnd,
        }); 

        let promise_reply = client.handle_prepare(prepare_request).await?;
        let promise = promise_reply.into_inner();
        println!("Received promise: {:?}", promise);
        promises.push(Promise {
            to: promise.to,
            from: promise.from,
            rnd: promise.rnd,
            vrnd: promise.vrnd,
            vval: promise.vval,
        });
    }

    println!("Promises: {:?}", promises);

    // PROPOSE
    println!("Handling promises one by one");
    let mut accepted_value = None;
    for promise in &promises {
        let accept = resource.call_handlepromise(&mut *store, my_proposer, promise)?;
        println!("Current promises: {:?}", resource.call_getnumpromises(&mut *store, my_proposer)?);
        if accept.from != 0 {
            println!("Accept: {:?}", accept);
            accepted_value = Some(accept);
            break;
        }
    }

    if let Some(accept) = accepted_value {
        println!("Quorum reached, sending accept messages");
        for client in &mut clients {
            let accept_request = Request::new(AcceptRequest {
                from: accept.from,
                rnd: accept.rnd,
                val: accept.val.clone(),
            });
            let learn_reply = client.handle_accept(accept_request).await?;
            println!("Accept reply: {:?}", learn_reply.into_inner());
        }
    } else {
        println!("Quorum not reached, aborting");
    }
    
    println!("Nothing broke");

    Ok(())
}