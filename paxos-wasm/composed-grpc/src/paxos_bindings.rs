// TODO: Create a shared package for this
wasmtime::component::bindgen! {{
    path: "../shared/wit/paxos.wit",
    world: "paxos-world",
    async: true,
    // TODO: Try async again later
}}
