use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ======================
    // Determine Workspace Root
    // ======================
    // The manifest directory here is the grpc package directory.
    // We want to go up one level to the workspace root.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = manifest_dir
        .parent()
        .expect("Failed to find workspace root")
        .to_owned();
    println!("Workspace Dir: {}", workspace_dir.to_str().unwrap());

    // TODO: This doesn't work due to nested Cargo Build command calls
    // // ======================
    // // WASM Component Builds
    // // ======================
    // // List the component package names and expected output wasm filenames.
    // let component_names = [
    //     "proposer-grpc",
    //     "acceptor-grpc",
    //     "learner-grpc",
    //     "kv-store-grpc",
    //     "paxos-grpc",
    // ];
    // let component_module_names = [
    //     "proposer_grpc.wasm",
    //     "acceptor_grpc.wasm",
    //     "learner_grpc.wasm",
    //     "kv_store_grpc.wasm",
    //     "paxos_grpc.wasm",
    // ];

    // println!("Building WASM components...");
    // // Build each component using Cargo. This assumes that each of these is a package in your workspace.
    // for component in &component_names {
    //     let status = Command::new("cargo")
    //         .current_dir(&workspace_dir)
    //         .args([
    //             "build",
    //             "-p",
    //             component, // Explicitly specify the package name.
    //             "--release",
    //             "--target",
    //             "wasm32-wasip2",
    //         ])
    //         .status()
    //         .expect(&format!("Failed to build crate {}", component));
    //     assert!(status.success(), "Failed to build crate {}", component);
    //     println!("Finished building {}", component);
    // }

    // // Check that the WASM artifacts exist and tell Cargo to re-run if they change.
    // let target_dir = workspace_dir
    //     .join("target")
    //     .join("wasm32-wasip2")
    //     .join("release");
    // for module in &component_module_names {
    //     let artifact_path = target_dir.join(module);
    //     assert!(
    //         artifact_path.exists(),
    //         "Required WASM module not found at: {}",
    //         artifact_path.display()
    //     );
    //     println!("cargo:rerun-if-changed={}", artifact_path.display());
    // }
    // // Also re-run if any of the source files for the components change.
    // for component in &component_names {
    //     let src_path = workspace_dir.join(component).join("src");
    //     println!("cargo:rerun-if-changed={}", src_path.display());
    // }

    // // ======================
    // // Composite WASM Component
    // // ======================
    // // Define file paths for each WASM module.
    // let proposer = target_dir.join("proposer_grpc.wasm");
    // let acceptor = target_dir.join("acceptor_grpc.wasm");
    // let learner = target_dir.join("learner_grpc.wasm");
    // let kv_store = target_dir.join("kv_store_grpc.wasm");
    // let paxos = target_dir.join("paxos_grpc.wasm");
    // let output = target_dir.join("composed_paxos_grpc.wasm");

    // // Run the composite command using wac plug.
    // // Command syntax:
    // //   wac plug --plug <proposer> --plug <acceptor> --plug <learner> --plug <kv_store> <paxos> -o <output>
    // let status = Command::new("wac")
    //     .arg("plug")
    //     .arg("--plug")
    //     .arg(proposer.to_str().unwrap())
    //     .arg("--plug")
    //     .arg(acceptor.to_str().unwrap())
    //     .arg("--plug")
    //     .arg(learner.to_str().unwrap())
    //     .arg("--plug")
    //     .arg(kv_store.to_str().unwrap())
    //     .arg(paxos.to_str().unwrap())
    //     .arg("-o")
    //     .arg(output.to_str().unwrap())
    //     .status()?;
    // if !status.success() {
    //     panic!("Failed to create composite WASM component using wac plug");
    // }
    // println!("Composite Paxos component created: {}", output.display());

    Ok(())
}
