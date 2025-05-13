use paxos_wasm_utils::build_helpers::{build_and_plug, build_wasm_components};
use std::env;

fn main() {
    // Cargo sets PROFILE to "debug" or "release"
    let profile = env::var("PROFILE").unwrap();
    if profile == "release" {
        // don’t do your wasm‑build work in `cargo build --release`
        println!("cargo:warning=Skipping wasm build in release profile");
        return;
    }

    let target = "wasm32-wasip2";

    // let base_components = &["serializer", "tcp-client", "udp-client"];
    let base_components = &["serializer", "tcp-client"];
    build_wasm_components(target, base_components);

    // --- Build and Plug Network-WS Components ---

    let build: &[&str] = &["network-server-tcp"];
    let plugs = &["serializer"];
    let socket = "network_server_tcp";
    let output = "composed_network_server_tcp";
    build_and_plug(target, build, plugs, socket, output);

    let build: &[&str] = &["client-server-tcp"];
    let plugs = &["serializer"];
    let socket = "client_server_tcp";
    let output = "composed_client_server_tcp";
    build_and_plug(target, build, plugs, socket, output);

    let build: &[&str] = &[];
    let plugs = &["serializer"];
    let socket: &str = "tcp_client";
    let output = "composed_tcp_client";
    build_and_plug(target, build, plugs, socket, output);

    // let build: &[&str] = &[];
    // let plugs = &["serializer"];
    // let socket = "udp_client";
    // let output = "composed_udp_client";
    // build_and_plug(target, build, plugs, socket, output);

    // --- Build WS plugged Components ---

    println!("BUILDING WASM OVERHEAD");
    let build_list = &[];
    let plugs = &["composed_network_server_tcp", "composed_tcp_client"];
    let socket = "wasm-overhead-wasm";
    let output = "composed_wasm_overhead";
    build_and_plug(target, build_list, plugs, socket, output);

    let build_list_agents: &[&str] = &[];
    let plugs = &["composed_tcp_client"];
    let socket_proposer = "composed_proposer_agent";
    let output_proposer = "tcp_composed_proposer_agent";
    build_and_plug(
        target,
        build_list_agents,
        plugs,
        socket_proposer,
        output_proposer,
    );

    let socket_acceptor = "composed_acceptor_agent";
    let output_acceptor = "tcp_composed_acceptor_agent";
    build_and_plug(
        target,
        build_list_agents,
        plugs,
        socket_acceptor,
        output_acceptor,
    );

    let socket_learner = "composed_learner_agent";
    let output_learner = "tcp_composed_learner_agent";
    build_and_plug(
        target,
        build_list_agents,
        plugs,
        socket_learner,
        output_learner,
    );

    // let build_list_agents: &[&str] = &[];
    // let plugs_agents = &["composed_udp_client"];
    // let socket_proposer = "composed_proposer_agent";
    // let output_proposer = "udp_composed_proposer_agent";
    // build_and_plug(
    //     target,
    //     build_list_agents,
    //     plugs_agents,
    //     socket_proposer,
    //     output_proposer,
    // );

    // let socket_acceptor = "composed_acceptor_agent";
    // let output_acceptor = "udp_composed_acceptor_agent";
    // build_and_plug(
    //     target,
    //     build_list_agents,
    //     plugs_agents,
    //     socket_acceptor,
    //     output_acceptor,
    // );

    // let socket_learner = "composed_learner_agent";
    // let output_learner = "udp_composed_learner_agent";
    // build_and_plug(
    //     target,
    //     build_list_agents,
    //     plugs_agents,
    //     socket_learner,
    //     output_learner,
    // );
}
