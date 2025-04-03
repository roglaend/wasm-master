use paxos_wasm_utils::build_helpers::build_wasm_components;

fn main() {
    let target = "wasm32-wasip2";
    let components = &["proposer", "acceptor", "learner", "kv-store"];
    build_wasm_components(target, components);
}
