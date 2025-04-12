use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Returns the workspace root directory.
/// Assumes the build script is in a crate three levels below the workspace root.
pub fn get_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("Failed to determine workspace root")
        .to_owned()
}

/// Returns the target directory given a target triple.
/// The workspace is determined automatically.
pub fn get_target_dir(target_triple: &str) -> PathBuf {
    get_workspace_root().join("target").join(target_triple)
}

/// Builds the specified crates (WASM components) in release mode for the given target triple.
/// The expected WASM file is derived from the crate name by replacing hyphens with underscores.
pub fn build_crates(workspace: &Path, target_dir: &Path, target_triple: &str, crates: &[&str]) {
    let release_dir = target_dir.join("release");
    for pkg in crates {
        let wasm_file = format!("{}.wasm", pkg.replace('-', "_"));
        let output_path = release_dir.join(&wasm_file);
        println!("Building `{}` -> {}", pkg, output_path.display());
        let status = Command::new("cargo")
            .current_dir(workspace)
            .args(&["build", "-p", pkg, "--release", "--target", target_triple])
            .status()
            .unwrap_or_else(|e| panic!("Failed to execute cargo build for {}: {}", pkg, e));
        assert!(
            status.success(),
            "cargo build failed for {} (status: {})",
            pkg,
            status
        );
        println!("Built `{}`", wasm_file);
    }
}

/// Validates that the WASM artifacts exist and informs Cargo to rerun if they change.
pub fn validate_artifacts(target_dir: &Path, crates: &[&str]) {
    let release_dir = target_dir.join("release");
    for pkg in crates {
        let wasm_file = format!("{}.wasm", pkg.replace('-', "_"));
        let artifact_path = release_dir.join(&wasm_file);
        assert!(
            artifact_path.exists(),
            "Required WASM module not found at: {}",
            artifact_path.display()
        );
        println!("cargo:rerun-if-changed={}", artifact_path.display());
    }
}

/// Instructs Cargo to rerun if any source file in the given crate directories changes.
pub fn rerun_if_source_changed(workspace: &Path, crates: &[&str]) {
    for pkg in crates {
        println!(
            "cargo:rerun-if-changed={}",
            workspace.join(pkg).join("src").display()
        );
    }
}

/// Convenience function to build WASM components:
/// Automatically determines the workspace and target directory, builds the crates,
/// validates that the artifacts exist, and emits Cargo rerun instructions.
pub fn build_wasm_components(target_triple: &str, crates: &[&str]) {
    let workspace = get_workspace_root();
    let target_dir = get_target_dir(target_triple);
    build_crates(&workspace, &target_dir, target_triple, crates);
    validate_artifacts(&target_dir, crates);
    rerun_if_source_changed(&workspace, crates);
}

/// Plugs the given modules together using `wac plug` to produce a composite WASM module.
///
/// - `plugs`: A slice of module names to be used as plug components.
/// - `socket`: The module name to be used as the socket component.
/// - `output`: The name of the composite output module.
pub fn plug_modules(target_dir: &Path, plugs: &[&str], socket: &str, output: &str) {
    let release_dir = target_dir.join("release");
    println!("Running wac plug for output `{}`", output);

    let mut args = vec!["plug".to_string()];

    // Process the plug modules.
    for &module in plugs {
        let path = release_dir.join(format!("{}.wasm", module));
        assert!(path.exists(), "Missing plug module: {}", path.display());
        args.push("--plug".into());
        args.push(path.to_string_lossy().into());
    }

    // Append the socket module.
    let socket_path = release_dir.join(format!("{}.wasm", socket));
    assert!(
        socket_path.exists(),
        "Missing socket module: {}",
        socket_path.display()
    );
    args.push(socket_path.to_string_lossy().into());
    args.push("-o".into());
    let output_path = release_dir.join(format!("{}.wasm", output));
    args.push(output_path.to_string_lossy().into());

    println!("Command: wac {}", args.join(" "));
    let status = Command::new("wac")
        .args(&args)
        .status()
        .unwrap_or_else(|e| panic!("Failed to execute wac plug for {}: {}", output, e));
    assert!(
        status.success(),
        "wac plug failed for {} (status: {})",
        output,
        status
    );
    println!("Composite module `{}` created", output);
}

/// Convenience function to build and plug WASM components into a composite module:
/// - `target_triple`: The target triple (e.g. "wasm32-wasip2").
/// - `build_list`: The list of crates to build.
/// - `plugs`: The list of plug module names.
/// - `socket`: The socket module name.
/// - `output`: The output composite module name.
pub fn build_and_plug(
    target_triple: &str,
    build_list: &[&str],
    plugs: &[&str],
    socket: &str,
    output: &str,
) {
    let workspace = get_workspace_root();
    let target_dir = get_target_dir(target_triple);
    build_crates(&workspace, &target_dir, target_triple, build_list);
    validate_artifacts(&target_dir, build_list);
    // rerun_if_source_changed(&workspace, build_list);
    plug_modules(&target_dir, plugs, socket, output);
}
