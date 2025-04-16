use clap::Parser;
use proto::paxos_proto;
use proto::paxos_proto::paxos_client::PaxosClient;

#[derive(Parser)]
struct Args {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = PaxosClient::connect("http://127.0.0.1:50057").await?;

    // let mut learner = PaxosClient::connect("http://127.0.0.1:50054").await?;

    let my_id = 1;
    let mut seq = 100;
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
        } else if input.eq_ignore_ascii_case("30") {
            for i in 0..30 {
                let request = tonic::Request::new(paxos_proto::ProposeRequest {
                    value: i.to_string(),
                    client_id: my_id,
                    client_seq: seq,
                });
                let _ = client.propose_value(request).await?;
            }
        } else {
            // Create a request from the user's input
            let request = tonic::Request::new(paxos_proto::ProposeRequest {
                value: input.to_string(),
                client_id: my_id,
                client_seq: seq,
            });

            seq += 1;

            let latency = std::time::Instant::now();

            let response = client.propose_value(request).await?;

            let elapsed = latency.elapsed();

            let rsp_msg = response.into_inner();

            if rsp_msg.success {
                println!("Response from server: {:?}", rsp_msg.message);
                println!("Latency: {:?}", elapsed);
            }
        }
    }
    Ok(())
}
