wasmtime::component::bindgen! {{
    path: "../shared/wit",
    world: "paxos-ws-world",
    additional_derives: [Clone],
    async: true,
    // TODO: Try async again later
}}
