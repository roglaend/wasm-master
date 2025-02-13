// use prost_types::{value, Struct};

use wasmtime_wasi::bindings::cli::environment::Host;
use std::env;
use std::sync::Arc;
// use std::sync::Mutex;
use tokio::sync::Mutex;

use tonic::{transport::Server};
pub mod host {
    tonic::include_proto!("host");
}

// For host
use host::handler_server::{Handler, HandlerServer};
use host::{HostRequest, HostResponse};
use host::handler_client::HandlerClient;

use wasmtime::component::bindgen;
use wasmtime::{Engine, Result, Store};
use wasmtime::component::{Component, Linker, ResourceAny};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView, ResourceTable};

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
        Self { 
            wasm_acceptor,
        }
    }
}

#[tonic::async_trait]
impl Handler for MyAcceptor {
    async fn handle_request(
        &self,
        request: tonic::Request<HostRequest>,
    ) -> Result<tonic::Response<HostResponse>, tonic::Status> {
        let req = request.into_inner();
        
        let mut store = self.wasm_acceptor.store.lock().await;
        let bindings = &self.wasm_acceptor.bindings;
        let guest = bindings.paxos_acceptor_acceptorinterface();
        // let resource = guest.acceptorresource();

        println!("Handling request: {:?}", req);

        let result = guest.call_handle_request(&mut *store, self.wasm_acceptor.resource_handle, &req.data);
        
        let (response, action) = result.unwrap();

        println!("After component has handled request");
        println!("Response: {:?}", response);
        println!("Action: {:?}", action);

        if action == "SEND_TO_ALL_CLIENTS" {
            // SEND TO ALL LEARNERNS
            // let responses = self.send_to_all_clients(response).await?;
            // println!("Responses: {:?}", responses);
        }

    
        let resp = HostResponse {
            data: response,
        };

        Ok(tonic::Response::new(resp))
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

    // let addr_str = format!("[::1]:{}", port);
    let addr_str = format!("0.0.0.0:{}", port);

    let addr = addr_str.parse()?;

    let wasm_acceptor = Arc::new(WasmAcceptor::new(acceptor_id)?);

    let acceptor_service = MyAcceptor::new(wasm_acceptor);

    println!("Acceptor listening on {}", addr);

    Server::builder()
        .add_service(HandlerServer::new(acceptor_service))
        .serve(addr)
        .await?;

    Ok(())
}