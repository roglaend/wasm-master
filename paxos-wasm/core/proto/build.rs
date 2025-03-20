use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = manifest_dir.join("src").join("generated");
    fs::create_dir_all(&out_dir)?;

    let proto_dir = manifest_dir.join("proto");
    let proto_file = proto_dir.join("paxos.proto");
    println!("Generating gRPC stubs into: {}", out_dir.display());

    tonic_build::configure()
        .out_dir(&out_dir)
        .build_server(true)
        .build_client(true)
        .compile(
            &[proto_file.to_str().unwrap()],
            &[proto_dir.to_str().unwrap()],
        )?;

    println!("cargo:rerun-if-changed={}", proto_file.display());
    Ok(())
}
