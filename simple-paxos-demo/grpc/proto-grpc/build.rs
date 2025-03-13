use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();

    println!("Workspace Dir: {}", workspace_dir.to_str().unwrap());

    println!("GRPC Build Script Is Running!");
    tonic_build::configure()
        .build_server(true) // Generate server stub
        .build_client(true) // Generate client stub
        .compile(
            &["./src/paxos.proto"], // Path to your .proto file(s)
            &["./src"],             // Path to the directory containing .proto
        )?;
    Ok(())
}
