use std::path::PathBuf;

use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker},
};
use wasmtime_wasi::{IoView, ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

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
                .allow_tcp(true)
                .build(),
            resource_table: ResourceTable::new(),
        }
    }
}

mod bindings {
    wasmtime::component::bindgen! {{
        path: "wit/tcp-server-polling.wit",
        world: "tcp-server-world",
        async: true,
    }}
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // Configure Wasmtime engine for async support and WASI
    let mut config = Config::default();
    config.async_support(true);
    config.wasm_component_model(true);

    let engine = Engine::new(&config).unwrap();

    let state = ComponentRunStates::new();

    let mut store = Store::new(&engine, state);
    let mut linker = Linker::<ComponentRunStates>::new(&engine);

    wasmtime_wasi::add_to_linker_async(&mut linker)?;

    let wasm_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-wasip2/release/composed_tcp_server_polling_test_server.wasm");
    let component = Component::from_file(&engine, wasm_path).unwrap();

    let bindings =
        bindings::TcpServerWorld::instantiate_async(&mut store, &component, &linker).await?;

    let tcp_server = bindings.poc_tcp_server_polling_tcp_server();

    println!("Running server and calling run on wasm component tcp server now...");

    tcp_server.call_run(&mut store).await?;

    Ok(())
}
