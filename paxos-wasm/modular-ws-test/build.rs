use paxos_wasm_utils::build_helpers::{build_and_plug, build_wasm_components};

fn main() {
    let target = "wasm32-wasip2";

    let base_components = &[
        "paxos-client-test",
    ];
    build_wasm_components(target, base_components);

    // --- Build and Plug WS Components ---

    let build_list_server: &[&str] = &[];
    let plugs_server = &["serializer"];
    let socket_server = "paxos_client_test";
    let output_server = "composed_paxos_client";
    build_and_plug(
        target,
        build_list_server,
        plugs_server,
        socket_server,
        output_server,
    );
;
}
