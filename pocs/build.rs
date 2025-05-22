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

    let target = "wasm32-wasip2";

    let build_list = &["wasm-overhead-wasm"];
    let plugs = &["composed_network_server_tcp", "composed_tcp_client"];
    let socket = "wasm_overhead_wasm";
    let output = "composed_wasm_overhead";
    build_and_plug(target, build_list, plugs, socket, output);
}
