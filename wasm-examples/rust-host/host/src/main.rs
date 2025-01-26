// use crate::utils::get_component_linker_store;
use wasmtime::component::bindgen;
// use wasmtime::{Engine, Result};
use anyhow::Context;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Engine, Result, Store};
use wasmtime_wasi::WasiImpl;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

// bindgen!({
//     world: "hosttest",
//     path: "../wit/hosttest",
// });

bindgen!("hosttest" in "../wit/hosttest/world.wit");

// pub(crate) struct ComponentRunStates {
//     // These two are required basically as a standard way to enable the impl of WasiView
//     pub wasi_ctx: WasiCtx,
//     pub resource_table: ResourceTable,
// }

// impl WasiView for ComponentRunStates {
//     fn table(&mut self) -> &mut ResourceTable {
//         &mut self.resource_table
//     }
//     fn ctx(&mut self) -> &mut WasiCtx {
//         &mut self.wasi_ctx
//     }
// }

// impl ComponentRunStates {
//     pub fn new() -> Self {
//         ComponentRunStates {
//             wasi_ctx: WasiCtxBuilder::new().build(),
//             resource_table: ResourceTable::new(),
//         }
//     }
// }

// pub fn get_component_linker_store(
//     engine: &Engine,
//     path: &'static str,
//     alt_path: &'static str,
// ) -> Result<(
//     Component,
//     Linker<ComponentRunStates>,
//     Store<ComponentRunStates>,
// )> {
//     let component = Component::from_file(engine, path)
//         .or_else(|_| Component::from_file(engine, alt_path))
//         .with_context(|| format!("Cannot find component from path: {path} or {alt_path}"))?;
//     let linker = Linker::new(engine);
//     let state = ComponentRunStates::new();
//     let store = Store::new(engine, state);
//     Ok((component, linker, store))
// }

fn main() -> Result<()> {
    let engine = Engine::default();
    let bytes = std::fs::read("../host-test/target/wasm32-wasip1/release/hosttest.wasm")?;
    let component = Component::new(&engine, bytes)?;
    let mut store = Store::new(&engine, ());
    let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
    // let export = component.get_export(None, "add").unwrap();
    let func = instance.get_func(&mut store, "add").expect("add import not found");
    // let func = instance.get_func(&mut store, &export).unwrap();
    let typed = func.typed::<(u32, u32), (u32,)>(&store)?;
    let res = typed.call(&mut store, (1, 2))?;
    // println!("type of export: {ty}");
    println!("return value was: {}", res.0);
    Ok(())
}