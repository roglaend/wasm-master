wasmtime::component::bindgen! {{
    path: "../../../paxos-wasm/shared/wit",
    world: "paxos-runner-world",
    additional_derives: [Clone],
    async: true,
    // TODO: Try async again later
}}
