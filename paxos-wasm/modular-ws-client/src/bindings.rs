wasmtime::component::bindgen! {{
    path: "../shared/wit",
    world: "paxos-client-world",
    additional_derives: [Clone],
    async: true,
    // TODO: Try async again later
}}
