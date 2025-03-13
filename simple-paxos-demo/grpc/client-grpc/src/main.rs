use clap::Parser;
use proto::paxos_proto;
use proto::paxos_proto::paxos_client::PaxosClient;

/// Command-line arguments for the client.
#[derive(Parser)]
struct Args {
    /// The address of the gRPC server (e.g. "http://127.0.0.1:50051")
    #[arg(long)]
    server_addr: String,
    /// Optional value to propose. If not provided, the client will request the current value.
    #[arg(long)]
    value: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command-line arguments.
    let args = Args::parse();

    // Connect to the gRPC server.
    let mut client = PaxosClient::connect(args.server_addr).await?;

    if let Some(val) = args.value {
        // If a value is provided, send a propose request.
        println!("Sending propose request with value: {}", val);
        let request = tonic::Request::new(paxos_proto::ProposeRequest { value: val });
        let response = client.propose_value(request).await?;
        println!("Propose response: {:?}", response.into_inner());
    } else {
        // Otherwise, just get the currently learned value.
        println!("Requesting current value...");
        let request = tonic::Request::new(paxos_proto::Empty {});
        let response = client.get_value(request).await?;
        let current_value = response.into_inner().value;
        println!("Current value: {:?}", current_value);
    }
    Ok(())
}
