use paxos_wasm_utils::build_helpers::{build_and_plug, build_wasm_components};

fn main() {
    let target = "wasm32-wasip2";

    // Build the base components. In this example we have:
    // - The Paxos agents and key-value storage,
    // - tcp-serializer,
    // - tcp-server, and
    // - tcp-client.
    let base_components = &[
        "proposer",
        "acceptor",
        "learner",
        "kv-store",
        "tcp-serializer",
        "tcp-server",
        "tcp-client",
    ];
    build_wasm_components(target, base_components);

    //* Build are package names. Plugs, socket and output are wasm component names. */
    let build_list = &["failure-detector", "leader-detector"];
    let plugs = &["leader_detector"];
    let socket = "failure_detector";
    let output = "composed_failure_service";
    build_and_plug(target, build_list, plugs, socket, output);

    // Now we need to plug tcp-serializer into both tcp-server and tcp-client.
    // Since these are already built in the base components, we can use an empty build list.

    // Build the composite TCP server module:
    // Plug tcp-serializer (as a plug module) into tcp-server (as the socket module),
    // producing a composite module named composed_tcp_server.
    let build_list_server: &[&str] = &[];
    let plugs_server = &["tcp_serializer"];
    let socket_server = "tcp_server";
    let output_server = "composed_tcp_server";
    build_and_plug(
        target,
        build_list_server,
        plugs_server,
        socket_server,
        output_server,
    );

    // Build the composite TCP client module:
    // Plug tcp-serializer (as a plug module) into tcp-client (as the socket module),
    // producing a composite module named composed_tcp_client.
    let build_list_client: &[&str] = &[];
    let plugs_client = &["tcp_serializer"];
    let socket_client = "tcp_client";
    let output_client = "composed_tcp_client";
    build_and_plug(
        target,
        build_list_client,
        plugs_client,
        socket_client,
        output_client,
    );
}
