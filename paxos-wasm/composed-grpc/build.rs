use std::path::PathBuf;
use std::process::Command;

fn main() {
    //     // Get the workspace root directory
    let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Failed to find workspace root")
        .to_owned();

    let component_names = ["paxos-coordinator"];

    let component_module_names = ["paxos_coordinator.wasm"];

    println!("Building WASM components...");

    // First, build each component explicitly
    for component_name in &component_names {
        let status = Command::new("cargo")
            .current_dir(&workspace_dir)
            .args([
                "build",
                "-p",
                component_name, // Explicitly specify the package
                "--release",
                "--target",
                "wasm32-wasip2",
            ])
            .status()
            .expect(&format!("Failed to build crate {}", component_name));

        assert!(status.success(), "Failed to build crate {}", component_name);
        println!("Finished building {}", component_name);
    }

    // Check if the WASM artifacts exist
    let target_dir = workspace_dir
        .parent()
        .expect("Should be the workspace target folder")
        .join("target")
        .join("wasm32-wasip2");

    for component_module_name in &component_module_names {
        let artifact_path = target_dir.join("release").join(component_module_name);
        assert!(
            artifact_path.exists(),
            "Required WASM module not found at: {}",
            artifact_path.display()
        );
        // Tell Cargo to rerun if the artifact changes
        println!("cargo:rerun-if-changed={}", artifact_path.display());
    }

    // Ensure Cargo rebuilds if the source files change
    for component_name in &component_names {
        println!(
            "cargo:rerun-if-changed={}",
            workspace_dir.join(component_name).join("src").display()
        );
    }

    // --- WAC Logic: Compose WASM modules using `wac plug` ---
    // Define file paths for each component.
    let proposer = target_dir.join("release").join("proposer.wasm");
    let acceptor = target_dir.join("release").join("acceptor.wasm");
    let learner = target_dir.join("release").join("learner.wasm");
    let kv_store = target_dir.join("release").join("kv_store.wasm");
    let paxos = target_dir.join("release").join("paxos_coordinator.wasm");
    let output = target_dir
        .join("release")
        .join("composed_paxos_coordinator.wasm");

    // Ensure all the required WASM files exist.
    for path in [&proposer, &acceptor, &learner, &kv_store, &paxos] {
        assert!(
            path.exists(),
            "Required WASM module not found: {}",
            path.display()
        );
    }

    println!("Running wac plug to create composite WASM module...");

    // Run the wac plug command with the proper arguments.
    let status = Command::new("wac")
        .args([
            "plug",
            "--plug",
            proposer.to_str().unwrap(),
            "--plug",
            acceptor.to_str().unwrap(),
            "--plug",
            learner.to_str().unwrap(),
            "--plug",
            kv_store.to_str().unwrap(),
            paxos.to_str().unwrap(), // The socket component.
            "-o",
            output.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to run wac plug command");

    assert!(status.success(), "wac plug command failed");

    println!("Composite Paxos component created: {}", output.display());
}
