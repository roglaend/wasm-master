use std::env;
use std::time::{Duration, Instant};

use futures::future::join_all;
use proto::paxos_proto;
use proto::paxos_proto::paxos_client::PaxosClient;

// use modular_grpc::proposer::grpc_service::LATENCIES_INSIDE_WASM;

/// Function representing a single client.
/// It connects to the leader and sends `num_requests` proposals.
async fn run_client(
    client_id: u64,
    num_requests: u64,
    leader_url: String,
) -> Result<Vec<Duration>, Box<dyn std::error::Error + Send + Sync + 'static>> {
    // Connect to the Paxos leader.
    let mut client = PaxosClient::connect(leader_url.clone()).await?;

    let mut latencies = Vec::with_capacity(num_requests as usize);

    for i in 0..num_requests {
        // Create a unique value per request.

        // value = set 1:i i
        // every client will set different keys to value of index
        let value = &format!("set {}:{} {}", client_id, i, i);
        let client_seq = i;

        let request = tonic::Request::new(paxos_proto::ProposeRequest {
            value: value.to_string(),
            client_id,
            client_seq,
        });

        // Record the start time.
        let start = Instant::now();

        // Send the propose request asynchronously.
        let response = client.propose_value(request).await?;

        // Calculate and log the elapsed time.
        let elapsed = start.elapsed();
        latencies.push(elapsed);
    }

    if !latencies.is_empty() {
        let total: u128 = latencies.iter().map(|d| d.as_micros()).sum();
        let client_average = total as f64 / latencies.len() as f64;
        println!(
            "Client {}: Average latency: {:.2} microseconds",
            client_id, client_average
        );
    }

    Ok(latencies)
}

async fn run_client_concurrent(
    client_id: u64,
    num_requests: u64,
    leader_url: String,
) -> Result<Vec<Duration>, Box<dyn std::error::Error + Send + Sync + 'static>> {
    // Connect once and later clone the client for each request.
    let client = PaxosClient::connect(leader_url).await?;

    // Create a vector to hold the spawned tasks (one per request).
    let mut request_tasks = Vec::with_capacity(num_requests as usize);

    for i in 0..num_requests {
        // Clone the client for concurrent use.
        let mut client_clone = client.clone();
        // Prepare the request.
        let value = &format!("set {}:{} {}", client_id, i, i);
        let client_seq = i;
        let request = tonic::Request::new(paxos_proto::ProposeRequest {
            value: value.to_string(),
            client_id,
            client_seq,
        });

        // Sleep for a short duration to simulate a delay between requests.
        // if i % 10 == 0 {
        tokio::time::sleep(Duration::from_micros(10)).await;
        // }
        request_tasks.push(tokio::spawn(async move {
            // Start the timer for this request.
            let start = Instant::now();
            // Send the propose request concurrently.
            let result = client_clone.propose_value(request).await;
            // You can optionally check the result here for errors.
            result.map(|_| start.elapsed())
        }));
    }

    // Await all the tasks concurrently.
    let results = join_all(request_tasks).await;

    let mut latencies = Vec::with_capacity(num_requests as usize);

    // Process each result.
    for res in results {
        match res {
            Ok(Ok(duration)) => {
                // Successfully sent the proposal and received a response.
                println!("Client {}: Recorded latency: {:?}", client_id, duration);
                latencies.push(duration);
            }
            Ok(Err(e)) => {
                eprintln!(
                    "Client {}: Request encountered an error: {:?}",
                    client_id, e
                );
            }
            Err(e) => {
                // This error means the spawned task itself panicked.
                eprintln!("Client {}: Task panicked: {:?}", client_id, e);
            }
        }
    }

    // Optionally compute per-client statistics.
    if !latencies.is_empty() {
        let total: u128 = latencies.iter().map(|d| d.as_micros()).sum();
        let average = total as f64 / latencies.len() as f64;
        println!(
            "Client {}: Average latency: {:.2} microseconds over {} requests",
            client_id,
            average,
            latencies.len()
        );
    }

    Ok(latencies)
}

/// Entry point that spawns several client tasks concurrently.
/// Each client sends a number of requests to the Paxos leader.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let nodeid: u8 = args[1]
        .parse()
        .expect("Failed to parse node id as a number");
    let concurrrent: u8 = args[2]
        .parse()
        .expect("Failed to parse concurrent as a number");
    let concurrent = concurrrent == 1;

    // Configuration
    let leader_url = format!("http://127.0.0.1:5005{}", nodeid);
    let num_clients = 3;
    let requests_per_client = 100;

    // Vector to hold all spawned client tasks.
    let mut tasks = Vec::with_capacity(num_clients as usize);

    let start_time = Instant::now();

    if concurrent {
        for client_id in 0..num_clients {
            let url = leader_url.to_string();
            tasks.push(tokio::spawn(async move {
                // Each client returns its vector of latencies.
                let result = run_client_concurrent(client_id, requests_per_client, url).await;
                (client_id, result)
            }));
        }
    } else {
        for client_id in 0..num_clients {
            let url = leader_url.to_string();
            tasks.push(tokio::spawn(async move {
                // Each client returns its vector of latencies.
                let result = run_client(client_id, requests_per_client, url).await;
                (client_id, result)
            }));
        }
    }

    // Aggregate latency results from all client tasks.
    let mut all_latencies = Vec::new();

    for task in tasks {
        match task.await {
            Ok((client_id, Ok(latencies))) => {
                println!(
                    "Client {} completed successfully with {} requests.",
                    client_id,
                    latencies.len()
                );
                all_latencies.extend(latencies);
            }
            Ok((client_id, Err(e))) => {
                eprintln!("Client {} encountered an error: {:?}", client_id, e);
            }
            Err(e) => {
                eprintln!("A client task panicked: {:?}", e);
            }
        }
    }

    let elapsed_time = start_time.elapsed();
    println!("Total elapsed time for all clients: {:?}", elapsed_time);

    // Compute and display overall metrics if any measurements were recorded.
    if !all_latencies.is_empty() {
        let total_count = all_latencies.len();
        let total: u128 = all_latencies.iter().map(|d| d.as_millis()).sum();
        let avg_latency = total as f64 / total_count as f64;
        let min_latency = all_latencies.iter().min().unwrap();
        let max_latency = all_latencies.iter().max().unwrap();

        println!("\nOverall metrics:");
        println!("Total requests: {}", total_count);
        println!("Total latency: {:?}", total);
        println!("Average latency: {:?}", avg_latency);
        println!("Minimum latency: {:?}", min_latency);
        println!("Maximum latency: {:?}", max_latency);
    } else {
        println!("No successful latency measurements recorded.");
    }

    Ok(())
}
