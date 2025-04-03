use paxos_wasm_utils::build_helpers::{build_and_plug, build_wasm_components};

fn main() {
    let target = "wasm32-wasip2";

    let base_components = &["proposer", "acceptor", "learner", "kv-store"];
    build_wasm_components(target, base_components);

    //* Build are package names. Plugs, socket and output are wasm component names. */
    let build_list = &["failure-detector", "leader-detector"];
    let plugs = &["leader_detector"];
    let socket = "failure_detector";
    let output = "composed_failure_service";
    build_and_plug(target, build_list, plugs, socket, output);
}
