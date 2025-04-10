use paxos_wasm_utils::build_helpers::{build_wasm_components, get_target_dir, plug_modules};

fn main() {
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

    // Compute the target directory using the helper.
    let target_dir = get_target_dir(target_triple);

    // Define plug jobs with a clear separation:
    //   - plugs: a slice of plug module names,
    //   - socket: the module to be used as the socket,
    //   - output: the name of the composite module.
    let plug_jobs: &[(&[&str], &str, &str)] = &[
        (&["proposer"], "proposer_agent", "temp_composed_proposer_agent"),
        (&["acceptor"], "acceptor_agent", "composed_acceptor_agent"),
        ( &["learner"], "learner_agent", "composed_learner_agent" ),
        ( &["composed_failure_service"], "temp_composed_proposer_agent", "composed_proposer_agent" ),
        // ( &["kv_store"], "kv_store_agent", "composed_kv_store_agent" ),
    ];

    println!("Plugging core components into agents…");
    for (plugs, socket, output) in plug_jobs {
        println!(
            "Plugging plugs: {:?} with socket: {} -> {}",
            plugs, socket, output
        );
        plug_modules(&target_dir, plugs, socket, output);
        println!(
            "Created {}",
            target_dir
                .join("release")
                .join(format!("{}.wasm", output))
                .display()
        );
    }
    println!("All compose jobs complete");
}
