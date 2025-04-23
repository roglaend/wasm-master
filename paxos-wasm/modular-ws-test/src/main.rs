mod bindings;
mod paxos_wasm;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::error::Error;

use bindings::paxos::default::network_types::Value;
use clap::Parser;
use paxos_wasm::{ComponentRunStates, PaxosWasmtime};
use tokio;
use tokio::task::JoinHandle;
use wasmtime::component::{Component, Linker};
use wasmtime::Engine;


type AnyError = Box<dyn Error + Send + Sync + 'static>;

/// Runs a certain number of Paxos requests (`num_requests`).
/// Returns two vectors: `(init_latencies, request_latencies)`.
async fn run_requests(
    engine: &Engine,
    pre: &bindings::PaxosClientWorldPre<ComponentRunStates>,
    client_id: u64,
    leader_address: &str,
    num_requests: usize,
) -> Result<(Vec<Duration>, Vec<Duration>), AnyError> {
    let mut init_latencies = Vec::with_capacity(num_requests);
    let mut request_latencies = Vec::with_capacity(num_requests);

    for i in 0..num_requests {
        // Measure initialization
        let init_start = Instant::now();
        let client = PaxosWasmtime::new(engine, pre.clone()).await?;
        let init_elapsed = init_start.elapsed();

        // Build a Paxos command
        let command = format!("set {}:{} foobar", client_id, i);
        let value = Value {
            is_noop: false,
            client_id,
            client_seq: i as u64,
            command: Some(command),
        };

        // Measure request latency
        let request_start = Instant::now();
        let _res = client.perform_request(leader_address.to_string(), value).await?;
        let request_elapsed = request_start.elapsed();

        init_latencies.push(init_elapsed);
        request_latencies.push(request_elapsed);
    }

    Ok((init_latencies, request_latencies))
}


#[derive(Parser, Debug)]
struct Args {
    /// Person's name to greet
    #[arg(short, long)]
    client_id: u64,

    /// Number of times to greet
    #[arg(short, long, default_value_t = 1)]
    num_requests: u64,
}

#[tokio::main]
async fn main() -> Result<(), AnyError> {

    let args = Args::parse();

    let client_id = args.client_id;
    let num_requests = args.num_requests as usize;

    println!("Client ID: {}", client_id);
    println!("Number of requests: {}", num_requests);

    // 1) Create an async-supporting Wasmtime engine
    let mut config = wasmtime::Config::default();
    config.async_support(true);
    let engine = Engine::new(&config)?;


    let leader_address = "127.0.0.1:7777"; // For udp 
    // let leader_address = "127.0.0.1:50057"; // For tcp

    // 3) Load your WASM component
    let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Must have a parent")
        .parent()
        .expect("Workspace folder")
        .to_owned();

    let composed_component = Component::from_file(
        &engine,
        workspace_dir.join("target/wasm32-wasip2/release/composed_paxos_client.wasm"),
    )?;

    // 4) Instantiate the linker + your Paxos client "Pre" state
    let mut linker = Linker::<ComponentRunStates>::new(&engine);
    wasmtime_wasi::add_to_linker_async(&mut linker)?;
    let pre_instance = linker.instantiate_pre(&composed_component)?;
    let paxos_client_pre = bindings::PaxosClientWorldPre::new(pre_instance)?;

    let arc_engine = Arc::new(engine);

    let time = Instant::now();

    let all_results = run_requests(&arc_engine, &paxos_client_pre, client_id, leader_address, num_requests).await?;

    let elapsed = time.elapsed();


    println!("Total elapsed time for all requests: {:?}", elapsed);

    println!("=== Final Summary ===");
    summarize_latencies("Initialization", &all_results.0);
    summarize_latencies("Request", &all_results.1);

    Ok(())
}

/// Helper function to print total, average, min, and max latencies
fn summarize_latencies(label: &str, durations: &[Duration]) {
    if durations.is_empty() {
        println!("No {} latencies recorded.", label);
        return;
    }
    let total_count = durations.len();
    let total: Duration = durations.iter().copied().sum();
    let avg = total / (total_count as u32);
    let min = durations.iter().min().unwrap();
    let max = durations.iter().max().unwrap();

    println!("{} Latencies:", label);
    println!("  Count = {}", total_count);
    println!("  Total = {:?}", total);
    println!("  Avg   = {:?}", avg);
    println!("  Min   = {:?}", min);
    println!("  Max   = {:?}", max);
    println!();
}

// async fn run_simulations(
//     engine: Arc<Engine>,
//     pre: &bindings::PaxosClientWorldPre<ComponentRunStates>,
//     leader_address: &str,
//     num_requests: usize,
//     concurrency: usize,
// ) -> Result<Vec<(Vec<Duration>, Vec<Duration>)>, AnyError> {
//     let mut handles = Vec::with_capacity(concurrency);

//     for i in 0..concurrency {
//         let e_clone = Arc::clone(&engine);
//         let pre_clone = pre.clone();
//         let leader_addr_str = leader_address.to_string(); // For moving into task
//         let client_id = i as u64;

//         let handle: JoinHandle<Result<(Vec<Duration>, Vec<Duration>), AnyError>> =
//             tokio::spawn(async move {
//                 run_requests(&e_clone, &pre_clone, client_id, &leader_addr_str, num_requests).await
//             });
//         handles.push(handle);
//     }

//     // Collect results from each parallel task
//     let mut results = Vec::with_capacity(concurrency);
//     for handle in handles {
//         let (init_lats, request_lats) = handle.await??;
//         results.push((init_lats, request_lats));
//     }

//     Ok(results)
// }
