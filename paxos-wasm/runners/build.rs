use paxos_wasm_utils::build_helpers::build_and_plug;
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

    // List of crates to build.
    let build_list = ["paxos-coordinator"];

    // For plugging, specify the plug modules and the socket module separately.
    // Here, "plugs" are the modules to plug in, and "socket" is the primary module.
    let plugs = [
        "composed_proposer_agent",
        "composed_acceptor_agent",
        "composed_learner_agent",
        "composed_failure_service",
    ];
    let socket = "paxos_coordinator";

    // The output composite WASM module will be named composed_paxos_coordinator.wasm.
    let output = "composed_paxos_coordinator";

    build_and_plug(target_triple, &build_list, &plugs, socket, output);
}
