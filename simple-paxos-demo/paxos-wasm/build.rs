// use std::path::PathBuf;
// use std::process::Command;

fn main() {
//     // Get the workspace root directory
//     let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
//         .parent()
//         .expect("Failed to find workspace root")
//         .to_owned();

//     let component_names = ["proposer", "acceptor", "learner"];

//     let component_module_names = ["proposer.wasm", "acceptor.wasm", "learner.wasm"];

//     println!("Building WASM components...");

//     // First, build each component explicitly
//     for component_name in &component_names {
//         let status = Command::new("cargo")
//             .current_dir(&workspace_dir)
//             .args([
//                 "build",
//                 "-p",
//                 component_name, // Explicitly specify the package
//                 "--release",
//                 "--target",
//                 "wasm32-wasip2",
//             ])
//             .status()
//             .expect(&format!("Failed to build crate {}", component_name));

//         assert!(status.success(), "Failed to build crate {}", component_name);
//         println!("Finished building {}", component_name);
//     }

//     // Check if the WASM artifacts exist
//     let target_dir = workspace_dir.join("target").join("wasm32-wasip2");

//     for component_module_name in &component_module_names {
//         let artifact_path = target_dir.join("release").join(component_module_name);
//         assert!(
//             artifact_path.exists(),
//             "Required WASM module not found at: {}",
//             artifact_path.display()
//         );
//         // Tell Cargo to rerun if the artifact changes
//         println!("cargo:rerun-if-changed={}", artifact_path.display());
//     }

//     // Ensure Cargo rebuilds if the source files change
//     for component_name in &component_names {
//         println!(
//             "cargo:rerun-if-changed={}",
//             workspace_dir.join(component_name).join("src").display()
//         );
//     }
}
