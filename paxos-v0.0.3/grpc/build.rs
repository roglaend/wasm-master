fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true) // Generate server stub
        .build_client(true) // Generate client stub
        .compile(
            &["proto/host.proto"], // Path to your .proto file(s)
            &["proto"],                  // Path to the directory containing .proto
        )?;
    Ok(())
}