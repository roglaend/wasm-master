use paxos_wasm_utils::build_helpers::{build_and_plug, build_wasm_components};

fn main() {
    let target = "wasm32-wasip2";

    let base_components = &["tcp-serializer", "tcp-server", "tcp-client"];
    build_wasm_components(target, base_components);

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

    let build_list: &[&str] = &[];
    let plugs = &["composed_tcp_client"];
    let socket = "composed_proposer_agent";
    let output = "tcp_composed_proposer_agent";
    build_and_plug(target, build_list, plugs, socket, output);

    let build_list: &[&str] = &[];
    let plugs = &["composed_tcp_client"];
    let socket = "composed_acceptor_agent";
    let output = "tcp_composed_acceptor_agent";
    build_and_plug(target, build_list, plugs, socket, output);

    let build_list: &[&str] = &[];
    let plugs = &["composed_tcp_client"];
    let socket = "composed_learner_agent";
    let output = "tcp_composed_learner_agent";
    build_and_plug(target, build_list, plugs, socket, output);

    let plugs = &[
        "tcp_composed_proposer_agent",
        "tcp_composed_acceptor_agent",
        "tcp_composed_learner_agent",
    ];
    let socket = "composed_tcp_server";
    let output = "final_composed_tcp_server";
    build_and_plug(&target, &[], plugs, socket, output);
}
