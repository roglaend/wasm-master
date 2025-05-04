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

    let base_components = &["tcp-server", "udp-server"];
    build_wasm_components(target, base_components);

    // --- Build and Plug Modular-WS Components ---

    let build_list_server: &[&str] = &[];
    let plugs_server = &["serializer"];
    let socket_server = "tcp_server";
    let output_server = "composed_tcp_server";
    build_and_plug(
        target,
        build_list_server,
        plugs_server,
        socket_server,
        output_server,
    );

    let build_list_server_udp: &[&str] = &[];
    let plugs_server_udp = &["serializer"];
    let socket_server_udp = "udp_server";
    let output_server_udp = "composed_udp_server";
    build_and_plug(
        target,
        build_list_server_udp,
        plugs_server_udp,
        socket_server_udp,
        output_server_udp,
    );

    // --- Compose the Final Server ---

    let plugs_final_tcp = &[
        "tcp_composed_proposer_agent",
        "tcp_composed_acceptor_agent",
        "tcp_composed_learner_agent",
        "tcp_composed_paxos_coordinator",
    ];
    let socket_final_tcp = "composed_tcp_server";
    let output_final_tcp = "final_composed_tcp_server";
    build_and_plug(
        target,
        &[],
        plugs_final_tcp,
        socket_final_tcp,
        output_final_tcp,
    );

    let plugs_final_udp = &[
        "udp_composed_proposer_agent",
        "udp_composed_acceptor_agent",
        "udp_composed_learner_agent",
        // "udp_composed_paxos_coordinator", // TODO
    ];
    let socket_final_udp = "composed_udp_server";
    let output_final_udp = "final_composed_udp_server";
    build_and_plug(
        target,
        &[],
        plugs_final_udp,
        socket_final_udp,
        output_final_udp,
    );
}
