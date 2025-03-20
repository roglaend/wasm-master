use tonic::{transport::Server};
use std::env;
use std::sync::Arc;
// use std::sync::Mutex;
use tokio::sync::Mutex;

pub mod paxos {
    tonic::include_proto!("paxos");
}

use paxos::acceptor_server::{Acceptor, AcceptorServer};
use paxos::{PrepareRequest, PrepareReply, AcceptRequest, AcceptReply};

use wasmtime::component::bindgen;
use wasmtime::{Engine, Result, Store};
use wasmtime::component::{Component, Linker};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView, ResourceTable};

use crate::exports::paxos::acceptor::acceptorinterface::Prepare;
use crate::exports::paxos::acceptor::acceptorinterface::Accept;
use wasmtime::component::ResourceAny;

bindgen!({
    path: "../wit/acceptor/world.wit",
    world: "acceptorworld"
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

pub struct WasmAcceptor {
    store: Mutex<Store<ComponentRunStates>>,
    bindings: Acceptorworld,
    resource_handle: ResourceAny,
}

impl WasmAcceptor {
    pub fn new(id: u32) -> Result<Self> {
        let engine = Engine::default();
        let component = Component::from_file(&engine, "../acceptor/target/wasm32-wasip1/release/acceptor.wasm")?;
        
        let mut linker = Linker::<ComponentRunStates>::new(&engine);
        
        wasmtime_wasi::add_to_linker_sync(&mut linker)?;

        let state = ComponentRunStates::new();
        
        let mut store = Store::new(&engine, state);

        let bindings = Acceptorworld::instantiate(&mut store, &component, &linker)?;

        let guest = bindings.paxos_acceptor_acceptorinterface();
        let resource = guest.acceptorresource();

        let resource_handle = resource.call_constructor(&mut store, id)?;

        Ok(Self {
            store: Mutex::new(store),
            bindings,
            resource_handle,
        })
    }
}

pub struct MyAcceptor {
    wasm_acceptor: Arc<WasmAcceptor>
}

impl MyAcceptor {
    pub fn new(wasm_acceptor: Arc<WasmAcceptor>) -> Self {
        Self { wasm_acceptor }
    }
}

#[tonic::async_trait]
impl Acceptor for MyAcceptor {
    async fn handle_prepare(
        &self,
        request: tonic::Request<PrepareRequest>,
    ) -> Result<tonic::Response<PrepareReply>, tonic::Status> {
        let req = request.into_inner();

    
        println!("Got handle prepare request: {:?}", req);

        let prepare = Prepare {
            from: req.from,
            crnd: req.crnd,
        };

        //
        let mut store = self.wasm_acceptor.store.lock().await;
        let bindings = &self.wasm_acceptor.bindings;

        let guest = bindings.paxos_acceptor_acceptorinterface();
        let resource = guest.acceptorresource();
       
        // let promise = resource.call_handleprepare(&mut *store, self.wasm_acceptor.resource_handle, prepare).unwrap();
        let promise = resource.call_handleprepare(&mut *store, self.wasm_acceptor.resource_handle, prepare)
        .map_err(|e| tonic::Status::internal(format!("WASM error: {:?}", e)))?;


        if promise.rnd == 0 {
            println!("Ignoring because of old round")
        } 
        
        println!("Returning promise: {:?}", promise);
        let reply = PrepareReply {
            to: promise.to,
            from: promise.from,
            rnd: promise.rnd,
            vrnd: promise.vrnd,
            vval: promise.vval,

        };
        Ok(tonic::Response::new(reply))
    }

    async fn handle_accept(
        &self,
        request: tonic::Request<AcceptRequest>,
    ) -> Result<tonic::Response<AcceptReply>, tonic::Status> {
        let req = request.into_inner();

        println!("Got handle accept request: {:?}", req);

        let accept = Accept {
            from: req.from,
            rnd: req.rnd,
            val: req.val,
        };

        let mut store = self.wasm_acceptor.store.lock().await;
        let bindings = &self.wasm_acceptor.bindings;

        let guest = bindings.paxos_acceptor_acceptorinterface();
        let resource = guest.acceptorresource();

        let learn = resource.call_handleaccept(&mut *store, self.wasm_acceptor.resource_handle, &accept)
        .map_err(|e| tonic::Status::internal(format!("WASM error: {:?}", e)))?;
    
        if learn.from == 0 {
            println!("Ignoring because round is lower or already chosen value for round")
        }

        println!("Returning learn: {:?}", learn);
        let accept_reply = AcceptReply {
            from: learn.from,
            rnd: learn.rnd,
            val: learn.val,
        };
        
        Ok(tonic::Response::new(accept_reply))
    }
}



#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let acceptor_id: u32 = env::args()
        .nth(1)
        .unwrap_or_else(|| "1".to_string())
        .parse()
        .expect("Invalid acceptor ID (must be an integer).");

    // Read the port from the second argument, default to 50051 if not provided.
    let port = env::args()
        .nth(2)
        .unwrap_or_else(|| "50051".to_string());

    let addr_str = format!("[::1]:{}", port);
    let addr = addr_str.parse()?;

    let wasm_acceptor = Arc::new(WasmAcceptor::new(acceptor_id)?);

    let acceptor_service = MyAcceptor::new(wasm_acceptor);

    println!("Acceptor listening on {}", addr);

    Server::builder()
        .add_service(AcceptorServer::new(acceptor_service))
        .serve(addr)
        .await?;

    Ok(())
}