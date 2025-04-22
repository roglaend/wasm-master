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

    // Build the base components.
    let base_components = &["proposer", "acceptor", "learner", "kv-store"];
    build_wasm_components(target, base_components);

    //* Build are package names. Plugs, socket and output are wasm component names. */
    let build_list = &["failure-detector", "leader-detector"];
    let plugs = &["leader_detector"];
    let socket = "failure_detector";
    let output = "composed_failure_service";
    build_and_plug(target, build_list, plugs, socket, output);
}
