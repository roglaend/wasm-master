// The generated bindings
wasmtime::component::bindgen! {{
    path: "../wit",
    world: "paxos-types-world",
    additional_derives: [Clone],
    async: true,
}}

pub use exports::paxos::default as paxos;

// Declare the module
mod translation_layer;

pub use translation_layer::*;
