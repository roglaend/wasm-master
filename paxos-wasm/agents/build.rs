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

    println!("Building agent crates…");
    build_wasm_components(
        target_triple,
        &[
            "proposer-agent",
            "acceptor-agent",
            "learner-agent",
            "kv-store-agent",
        ],
    );
    println!("Agents built successfully\n");

    println!("Plugging core components into agents…");

    // Build composite proposer agent:
    // Plug "proposer" into "proposer_agent" to yield "temp_composed_proposer_agent".
    build_and_plug(
        target_triple,
        &[],
        &["proposer"],
        "proposer_agent",
        "temp_composed_proposer_agent",
    );
    println!("Created temp_composed_proposer_agent");

    // Build composite acceptor agent:
    build_and_plug(
        target_triple,
        &[],
        &["acceptor"],
        "acceptor_agent",
        "composed_acceptor_agent",
    );
    println!("Created composed_acceptor_agent");

    // Build composite learner agent:
    build_and_plug(
        target_triple,
        &[],
        &["learner"],
        "learner_agent",
        "composed_learner_agent",
    );
    println!("Created composed_learner_agent");

    // Re-plug the failure service composition:
    // Plug "composed_failure_service" into the previously built temp_composed_proposer_agent
    // to yield the final composed proposer agent.
    build_and_plug(
        target_triple,
        &[],
        &["composed_failure_service"],
        "temp_composed_proposer_agent",
        "composed_proposer_agent",
    );
    println!("Created composed_proposer_agent");

    println!("All compose jobs complete");
}
