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

    if env::var("CARGO_FEATURE_HOST_STORAGE").is_ok() {
        build_and_plug(
            target_triple,
            &["proposer-agent"],
            &["proposer"],
            "proposer_agent",
            "composed_proposer_agent",
        );
        println!("Created composed_proposer_agent");

        // Build composite acceptor agent:
        build_and_plug(
            target_triple,
            &["acceptor-agent"],
            &["acceptor"],
            "acceptor_agent",
            "composed_acceptor_agent",
        );
        println!("Created composed_acceptor_agent");

        // Build composite learner agent:
        build_and_plug(
            target_triple,
            &["learner-agent"],
            &["learner", "kv_store"],
            "learner_agent",
            "composed_learner_agent",
        );
        println!("Created composed_learner_agent");
    } else {
        // Build composite proposer agent:
        // Plug "proposer" into "proposer_agent" to yield "temp_composed_proposer_agent".
        build_and_plug(
            target_triple,
            &["proposer-agent"],
            &["proposer_storage"],
            "proposer_agent",
            "composed_proposer_agent",
        );
        println!("Created composed_proposer_agent");

        // Build composite acceptor agent:
        build_and_plug(
            target_triple,
            &["acceptor-agent"],
            &["acceptor_storage"],
            "acceptor_agent",
            "composed_acceptor_agent",
        );
        println!("Created composed_acceptor_agent");

        // Build composite learner agent:
        build_and_plug(
            target_triple,
            &["learner-agent"],
            &["learner_storage", "kv_store"],
            "learner_agent",
            "composed_learner_agent",
        );
        println!("Created composed_learner_agent");
    }

    println!("All compose jobs complete");
}
