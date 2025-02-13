use tonic::{client, Request};
use tokio::sync::Mutex;
use std::sync::Arc;
use std::env;



use tonic::{transport::Server};
pub mod host {
    tonic::include_proto!("host");
}

// // Import the generated client
// use paxos::acceptor_client::AcceptorClient;
// use paxos::{PrepareRequest, AcceptRequest};

// For host
use host::handler_server::{Handler, HandlerServer};
use host::handler_client::HandlerClient;
use host::{HostRequest, HostResponse};


use wasmtime::component::bindgen;
use wasmtime::{Engine, Result, Store};
use wasmtime::component::{Component, Linker, ResourceAny};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView, ResourceTable};


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
    pub fn new(id: u32, num_nodes: u32) -> Result<Self> {
        let engine = Engine::default();
        let component = Component::from_file(&engine, "../proposer/target/wasm32-wasip1/release/proposer.wasm")?;
        
        let mut linker = Linker::<ComponentRunStates>::new(&engine);
        wasmtime_wasi::add_to_linker_sync(&mut linker)?;

        let state = ComponentRunStates::new();
        let mut store = Store::new(&engine, state);

        let bindings = Proposerworld::instantiate(&mut store, &component, &linker)?;

        let guest = bindings.paxos_proposer_proposerinterface();
        let resource = guest.proposerresource();
        let resource_handle = resource.call_constructor(&mut store, id, num_nodes)?;

        Ok(Self {
            store: Mutex::new(store),
            bindings, 
            resource_handle,
        })
    }
}

#[derive(Clone)]
pub struct MyHandler {
    wasm_proposer: Arc<WasmProposer>,
    // ACCEPTOR CLIENTS
    clients: Arc<Mutex<Vec<HandlerClient<tonic::transport::Channel>>>>,
}


impl MyHandler {
    pub async fn new(wasm_proposer: Arc<WasmProposer>, acceptor_addresses: Vec<&str>) -> Self {
        // Build the Vec first in a local variable
        let clients = Arc::new(Mutex::new(Vec::new()));
        
        for address in acceptor_addresses {
            match HandlerClient::connect(address.to_string()).await {
                Ok(client) => clients.lock().await.push(client),
                Err(err) => eprintln!("Failed to connect to {}: {}", address, err),
            }
        }

        Self {
            wasm_proposer,
            clients,
        }
    }

    async fn send_to_all_clients(&self, req: String) -> Result<Vec<HostResponse>, tonic::Status> {
        let mut clients = self.clients.lock().await;
        let mut responses = Vec::new();
        for client in clients.iter_mut() {
            let request = Request::new(HostRequest {
                data: req.clone(),
            });
            let response = client.handle_request(request).await?.into_inner();

            responses.push(response);
        }
        Ok(responses)
    }
}




#[tonic::async_trait]
impl Handler for MyHandler {
    async fn handle_request(
        &self,
        request: tonic::Request<HostRequest>,
    ) -> Result<tonic::Response<HostResponse>, tonic::Status> {
        let req = request.into_inner();
        
        let mut store = self.wasm_proposer.store.lock().await;
        let bindings = &self.wasm_proposer.bindings;
        let guest = bindings.paxos_proposer_proposerinterface();


        println!("Handling request: {:?}", req);

        let result = guest.call_handle_request(&mut *store, self.wasm_proposer.resource_handle, &req.data);
        let (response, action) = result.unwrap();

        println!("After component has handled request");
        println!("Response: {:?}", response);
        println!("Action: {:?}", action);

        if action == "SEND_TO_ALL_CLIENTS_AND_HANDLE" {
            println!("Sending to all clients");
            let responses = &self.send_to_all_clients(response).await?;
            for response in responses {
                println!("Handliing responses in propopser component: {:?}", response);
                let result = guest.call_handle_request(&mut *store, self.wasm_proposer.resource_handle, &response.data);
                let (response, action) = result.unwrap();
                println!("Response: {:?}", response);
                println!("Action: {:?}", action);
                if action == "SEND_TO_ALL_CLIENTS" {
                    let _ = &self.send_to_all_clients(response).await;
                    break;
                }
            }
        }


        let resp = HostResponse {
            data: "Value proposed and ".to_string(),
        };

        Ok(tonic::Response::new(resp))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proposer_id: u32 = env::args()
        .nth(1)
        .unwrap_or_else(|| "1".to_string())
        .parse()
        .expect("Invalid proposer ID (must be an integer).");

    // Read the port from the second argument, default to 50051 if not provided.
    let port = env::args()
        .nth(2)
        .unwrap_or_else(|| "50041".to_string());

    // let addr_str = format!("[::1]:{}", port);
    let addr_str = format!("0.0.0.0:{}", port);

    let addr = addr_str.parse()?;

    let wasm_proposer = Arc::new(WasmProposer::new(proposer_id, 3)?);

    let acceptor_addresses = vec![
                    "http://0.0.0.0:50051",
                    "http://0.0.0.0:50052",
                    "http://0.0.0.0:50053",
                ];

    let proposer_service = MyHandler::new(wasm_proposer, acceptor_addresses).await;

    println!("Proposer listening on {}", addr);

    Server::builder()
        .add_service(HandlerServer::new(proposer_service))
        .serve(addr)
        .await?;

    Ok(())
}


// #[tokio::main]
// async fn main() -> Result<(), Box<dyn std::error::Error>> {
//     let proposer = Arc::new(WasmProposer::new()?);  

//     let acceptor_addresses = vec![
//         "http://[::1]:50051",
//         "http://[::1]:50052",
//         "http://[::1]:50053",
//     ];
    
//     let mut clients = Vec::new();
//     for address in &acceptor_addresses {
//         clients.push(AcceptorClient::connect(address.to_string()).await?);
//     }

//     // Lock store and use bindings safely
//     let mut store = proposer.store.lock().await;
//     let guest = proposer.bindings.paxos_proposer_proposerinterface();
//     let resource = guest.proposerresource();

//     let my_proposer = proposer.resource_handle;


//     // resource.call_increasecrnd(&mut *store, my_proposer)?;
//     // resource.call_requestvalue(&mut *store,  my_proposer, 987)?;
    
//     // PREPARE 
//     let mut promises = Vec::new();
//     for client in &mut clients {  
//         let prepare = resource.call_makeprepare(&mut *store, my_proposer)?;
//         println!("Sending prepare: {:?}", prepare);
//         let prepare_request = Request::new(PrepareRequest {
//             from: prepare.from,
//             crnd: prepare.crnd,
//         }); 

//         let promise_reply = client.handle_prepare(prepare_request).await?;
//         let promise = promise_reply.into_inner();
//         println!("Received promise: {:?}", promise);
//         promises.push(Promise {
//             to: promise.to,
//             from: promise.from,
//             rnd: promise.rnd,
//             vrnd: promise.vrnd,
//             vval: promise.vval,
//         });
//     }

//     println!("Promises: {:?}", promises);

//     // PROPOSE
//     println!("Handling promises one by one");
//     let mut accepted_value = None;
//     for promise in &promises {
//         let accept = resource.call_handlepromise(&mut *store, my_proposer, promise)?;
//         println!("Current promises: {:?}", resource.call_getnumpromises(&mut *store, my_proposer)?);
//         if accept.from != 0 {
//             println!("Accept: {:?}", accept);
//             accepted_value = Some(accept);
//             break;
//         }
//     }

//     if let Some(accept) = accepted_value {
//         println!("Quorum reached, sending accept messages");
//         for client in &mut clients {
//             let accept_request = Request::new(AcceptRequest {
//                 from: accept.from,
//                 rnd: accept.rnd,
//                 val: accept.val.clone(),
//             });
//             let learn_reply = client.handle_accept(accept_request).await?;
//             println!("Accept reply: {:?}", learn_reply.into_inner());
//         }
//     } else {
//         println!("Quorum not reached, aborting");
//     }
    
//     println!("Nothing broke");

//     Ok(())
// }