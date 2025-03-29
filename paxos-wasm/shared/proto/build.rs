use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Determine the manifest directory.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Set the output directory for generated code to "<manifest_dir>/src/generated".
    let out_dir = manifest_dir.join("src").join("generated");
    fs::create_dir_all(&out_dir)?;

    // Define the proto directory and the specific proto file.
    let proto_dir = manifest_dir.join("proto");
    let proto_file = proto_dir.join("paxos.proto");

    println!("Generating gRPC stubs into: {}", out_dir.display());

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .out_dir(&out_dir)
        .compile(
            &[proto_file.to_str().ok_or("Invalid proto file path")?],
            &[proto_dir.to_str().ok_or("Invalid proto directory path")?],
        )?;

    // Instruct Cargo to rerun this build script if the proto file or directory changes.
    println!("cargo:rerun-if-changed={}", proto_file.display());
    println!("cargo:rerun-if-changed={}", proto_dir.display());
    Ok(())
}
