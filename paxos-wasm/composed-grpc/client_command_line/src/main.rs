use clap::Parser;
use env_logger;
use log::info;
use proto::paxos_proto;
use proto::paxos_proto::paxos_client::PaxosClient;

#[derive(Parser)]
struct Args {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = PaxosClient::connect("http://127.0.0.1:50054").await?;

    loop {
        // Prompt the user for input
        println!("Enter a message (or 'quit' to exit):");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.eq_ignore_ascii_case("quit") {
            break;
        }

        if input.eq_ignore_ascii_case("log") {
            let request = tonic::Request::new(paxos_proto::Empty {});
            let response = client.get_state(request).await?;
            let state = response.into_inner();
            println!("Learned entries:");
            for entry in state.learned {
                println!("Slot {}: value = {:?}", entry.slot, entry.value);
            }
        } else {
            // Create a request from the user's input
            let request = tonic::Request::new(paxos_proto::ProposeRequest {
                value: input.to_string(),
            });
            let response = client.propose_value(request).await?;

            if response.into_inner().success {
                println!("Intitiated")
            }
        }
    }
    Ok(())
}
