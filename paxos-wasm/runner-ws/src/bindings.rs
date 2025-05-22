wasmtime::component::bindgen! {{
    path: "../shared/wit",
    world: "paxos-runner-world",
    additional_derives: [Clone],
    async: true,
    with: {
        "paxos:default/network-server/network-server-resource": crate::paxos_wasm::NetworkServerResource,
        "paxos:default/network-client/network-client-resource": crate::paxos_wasm::NetworkClientResource,
    }
}}
