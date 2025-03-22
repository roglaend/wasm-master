use std::path::PathBuf;
use std::process::Command;

fn main() {
    let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Expected parent")
        .parent()
        .expect("Expected to be inside a Cargo workspace")
        .to_owned();

    let component_names = [
        "tcp-server-polling-test-server",
        "tcp-server-polling-test-handler",
    ];
    let component_modules = [
        "tcp_server_polling_test_server.wasm",
        "tcp_server_polling_test_handler.wasm",
    ];

    println!("Building TCP server + handler components...");

    for component in &component_names {
        let status = Command::new("cargo")
            .current_dir(&workspace_dir)
            .args([
                "build",
                "--release",
                "--target",
                "wasm32-wasip2",
                "-p",
                component,
            ])
            .status()
            .expect(&format!("Failed to build component: {}", component));

        assert!(
            status.success(),
            "Build failed for component: {}",
            component
        );
    }

    let target_dir = workspace_dir
        .join("target")
        .join("wasm32-wasip2")
        .join("release");

    for module in &component_modules {
        let path = target_dir.join(module);
        assert!(
            path.exists(),
            "Expected WASM output not found: {}",
            path.display()
        );
        println!("cargo:rerun-if-changed={}", path.display());
    }

    // Tell Cargo to rerun if source files change
    for component in &component_names {
        let src_path = workspace_dir.join(component).join("src");
        println!("cargo:rerun-if-changed={}", src_path.display());
    }

    // Compose the server and handler using `wac plug`
    let server = target_dir.join("tcp_server_polling_test_server.wasm");
    let handler = target_dir.join("tcp_server_polling_test_handler.wasm");
    let output = target_dir.join("composed_tcp_server_polling_test_server.wasm");

    assert!(server.exists(), "Missing server WASM: {}", server.display());
    assert!(
        handler.exists(),
        "Missing handler WASM: {}",
        handler.display()
    );

    println!("Running `wac plug` to link server and handler...");

    let status = Command::new("wac")
        .args([
            "plug",
            "--plug",
            handler.to_str().unwrap(),
            server.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to run `wac plug`");

    assert!(status.success(), "`wac plug` command failed");
    println!("✅ Final composed component: {}", output.display());
}
