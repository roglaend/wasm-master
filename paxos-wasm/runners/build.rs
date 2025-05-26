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

    let target_triple = "wasm32-wasip2";

    if env::var("CARGO_FEATURE_HOST_NETWORK").is_ok() {
        let build_list = [];
        let plugs = [
            "composed_proposer_agent",
            "composed_acceptor_agent",
            "composed_learner_agent",
            "composed_failure_service",
        ];
        let socket = "paxos_coordinator";
        let output = "composed_paxos_coordinator";
        build_and_plug(target_triple, &build_list, &plugs, socket, output);

        let build_list = [];
        let plugs = [
            "composed_proposer_agent",
            "composed_acceptor_agent",
            "composed_learner_agent",
            "composed_failure_service",
        ];
        let socket = "paxos_coordinator";
        let output = "composed_paxos_coordinator";
        build_and_plug(target_triple, &build_list, &plugs, socket, output);

        let build_list = [];
        let plugs = [
            // "composed_network_server_tcp",
            "composed_client_server_tcp",
            "composed_paxos_coordinator",
        ];
        let socket = "runner";
        let output = "final_composed_runner";
        build_and_plug(target_triple, &build_list, &plugs, socket, output);
    } else {
        let build_list = [];
        let plugs = [
            "composed_proposer_agent",
            "composed_acceptor_agent",
            "composed_learner_agent",
            "composed_failure_service",
        ];
        let socket = "paxos_coordinator";
        let output = "composed_paxos_coordinator";
        build_and_plug(target_triple, &build_list, &plugs, socket, output);

        let build_list = [];
        let plugs = [
            "tcp_composed_proposer_agent",
            "tcp_composed_acceptor_agent",
            "tcp_composed_learner_agent",
            "composed_failure_service",
        ];
        let socket = "paxos_coordinator";
        let output = "tcp_composed_paxos_coordinator";
        build_and_plug(target_triple, &build_list, &plugs, socket, output);

        let build_list = [];
        let plugs = [
            "composed_network_server_tcp",
            "composed_client_server_tcp",
            "tcp_composed_paxos_coordinator",
        ];
        let socket = "runner";
        let output = "final_composed_runner";
        build_and_plug(target_triple, &build_list, &plugs, socket, output);
    }

    build_wasm_components(target_triple, &["paxos-coordinator", "runner"]);
}
