use clap::Parser;
use env_logger;
use log::info;
use proto::paxos_proto;
use proto::paxos_proto::paxos_client::PaxosClient;

#[derive(Parser)]
struct Args {
    /// The address of the gRPC server (e.g. "http://127.0.0.1:50051")
    #[arg(long)]
    server_addr: String,
    /// Optional value to propose. If not provided, the client will request the current Paxos state.
    #[arg(long)]
    value: Option<String>,
    /// Fetch log entries from the server instead of proposing a value or getting state.
    #[arg(long)]
    fetch_logs: bool,
    /// The last offset received from previous log queries (default 0).
    #[arg(long, default_value_t = 0)]
    last_offset: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let args = Args::parse();
    let mut client = PaxosClient::connect(args.server_addr).await?;

    if args.fetch_logs {
        info!(
            "Fetching logs from server with last_offset: {}",
            args.last_offset
        );
        let request = tonic::Request::new(paxos_proto::GetLogsRequest {
            last_offset: args.last_offset,
        });
        let response = client.get_logs(request).await?;
        let logs = response.into_inner();
        info!(
            "Fetched {} log entries, new_offset: {}",
            logs.entries.len(),
            logs.new_offset
        );
        for entry in logs.entries {
            info!("Offset {}: {}", entry.offset, entry.message);
        }
    } else if let Some(val) = args.value {
        info!("Sending propose request with value: {}", val);
        let request = tonic::Request::new(paxos_proto::ProposeRequest { value: val });
        let response = client.propose_value(request).await?;
        info!("Propose response: {:?}", response.into_inner());
    } else {
        info!("Requesting current Paxos state...");
        let request = tonic::Request::new(paxos_proto::Empty {});
        let response = client.get_state(request).await?;
        let state = response.into_inner();
        info!("Learned entries:");
        for entry in state.learned {
            info!("  Slot {}: value = {:?}", entry.slot, entry.value);
        }
        info!("KV-Store state:");
        for pair in state.kv_state {
            info!("  {} => {:?}", pair.key, pair.value);
        }
    }
    Ok(())
}
