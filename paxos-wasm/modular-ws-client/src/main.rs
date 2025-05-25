use std::path::PathBuf;
use std::time::{Duration, Instant};

use bindings::paxos::default::paxos_types::Operation;
use clap::{Parser, ValueEnum};
use tokio::time::sleep;
use wasmtime::Engine;
use wasmtime::component::{Component, Linker};
use wasmtime_wasi::add_to_linker_async;

mod bindings;
mod paxos_wasm;

use bindings::paxos::default::network_types::Value;
use paxos_wasm::{ComponentRunStates, PaxosWasmtime};

/// Choose either the stateless one-shot API or the stateful resource API.
#[derive(Clone, ValueEnum, Debug)]
enum ClientMode {
    Oneshot,
    Persistent,
}

#[derive(Parser)]
struct Args {
    #[clap(long)]
    client_id: u64,

    #[clap(long)]
    num_requests: usize,

    #[clap(long, value_enum, default_value = "oneshot")]
    mode: ClientMode,

    #[clap(long, default_value = "127.0.0.1:60000")]
    leader: String,

    #[clap(long, default_value = "5")]
    timeout_secs: u64,

    #[clap(long, default_value = "10")]
    num_logical_clients: usize,

    #[clap(long, default_value = "0")]
    client_id_offset: u64,
}

fn summarize(label: &str, durations: &[Duration]) {
    if durations.is_empty() {
        eprintln!("No {} recorded.", label);
        return;
    }
    let count = durations.len();
    let total: Duration = durations.iter().copied().sum();
    let avg = total / (count as u32);
    let min = *durations.iter().min().unwrap();
    let max = *durations.iter().max().unwrap();
    println!("=== {} Summary ===", label);
    println!("Count     = {}", count);
    println!("Total     = {:?}", total);
    println!("Avg       = {:?}", avg);
    println!("Min       = {:?}", min);
    println!("Max       = {:?}", max);
    println!();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!(
        "Mode={:?}, client={}, requests={}",
        args.mode, args.client_id, args.num_requests
    );
    let mut cfg = wasmtime::Config::default();
    cfg.async_support(true);
    let engine = Engine::new(&cfg)?;

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let component_wasm =
        workspace.join("target/wasm32-wasip2/release/composed_paxos_ws_client.wasm");
    let component = Component::from_file(&engine, component_wasm)?;

    let mut linker = Linker::<ComponentRunStates>::new(&engine);
    add_to_linker_async(&mut linker)?;
    let pre = linker.instantiate_pre(&component)?;
    let client_world = bindings::PaxosClientWorldPre::new(pre)?;

    let paxos = PaxosWasmtime::new(&engine, client_world.clone())
        .await
        .unwrap();

    match args.mode {
        ClientMode::Oneshot => {
            // ————— Oneshot —————
            let mut lats = Vec::with_capacity(args.num_requests);
            let start = Instant::now();
            for seq in 0..args.num_requests {
                let val = Value {
                    client_id: args.client_id.to_string(),
                    client_seq: seq as u64,
                    command: Some(Operation::Demo),
                };
                let t0 = Instant::now();
                let resp = paxos.perform_request(args.leader.clone(), val).await;
                let dt = t0.elapsed();
                println!("seq={} -> {:?} in {:?}", seq, resp, dt);
                lats.push(dt);
            }
            let wall = start.elapsed();
            let tput = args.num_requests as f64 / wall.as_secs_f64();
            println!("\nOneshot throughput: {:.2} req/sec\n", tput);
            summarize("Oneshot Latencies", &lats);
        }
        ClientMode::Persistent => {
            // ————— Persistent —————
            let num_logical_clients = args.num_logical_clients;
            let requests_per_client = args.num_requests as u64;

            let mut handles = Vec::new();

            for i in 0..num_logical_clients {
                let paxos = PaxosWasmtime::new(&engine, client_world.clone())
                    .await
                    .expect("failed to init client");

                let leader = args.leader.clone();
                let client_id = args.client_id_offset + i as u64;

                let handle = tokio::spawn(async move {
                    run_logical_client(
                        paxos,
                        client_id,
                        leader,
                        requests_per_client,
                        args.timeout_secs,
                    )
                    .await
                });

                handles.push(handle);
                sleep(Duration::from_millis(10)).await;
            }

            let mut all_latencies = Vec::new();
            for handle in handles {
                match handle.await {
                    Ok(Ok((_client_id, lats))) => {
                        all_latencies.extend(lats);
                    }
                    Ok(Err(e)) => {
                        println!("Client logic error: {:?}", e);
                    }
                    Err(e) => {
                        println!("Join error (task panicked): {:?}", e);
                    }
                }
            }
            for lat in &all_latencies {
                println!("{}", lat.as_millis());
            }
        }
    }

    Ok(())
}

async fn run_logical_client(
    paxos: PaxosWasmtime,
    client_id: u64,
    leader: String,
    num_requests: u64,
    timeout_secs: u64,
) -> Result<(u64, Vec<Duration>), Box<dyn std::error::Error + Send + Sync>> {
    let guest = paxos.bindings.paxos_default_paxos_client();
    let resource = guest.paxos_client_resource();
    let mut store = paxos.store.lock().await;
    let handle = resource.call_constructor(&mut *store).await?;

    let mut next = 0;
    let mut sent = std::collections::HashMap::new();
    let mut seen = std::collections::HashSet::new();
    let mut lats = Vec::with_capacity(num_requests as usize);

    let start = Instant::now();
    let deadline = start + Duration::from_secs(timeout_secs);
    let max_in_flight = 1; // ensures one round trip before sending the next request
    while (seen.len() as u64) < num_requests && Instant::now() < deadline {
        if next < num_requests && sent.len() < max_in_flight {
            let req = Value {
                client_id: format!("client-{}", client_id),
                client_seq: next,
                command: Some(Operation::Demo),
            };
            if resource
                .call_send_request(&mut *store, handle, &leader, &req)
                .await?
            {
                sent.insert(next, Instant::now());
                next += 1;
            } else {
                println!("Failed to send request");
            }
        }
        let last = next - 1;
        if let Some(time_sent) = sent.get(&last) {
            if time_sent.elapsed() > Duration::from_secs(3) {
                // Timeout long taking request, dont care
                sent.remove(&last);
            }
        }

        let (replies, success) = resource
            .call_try_receive(&mut *store, handle, &leader)
            .await?;

        // success returns true if we tried to receive successfully
        // if false, conncetion is closed. For now this only happens
        // when the leader restarts, so we retry the last request
        if !success {
            // if we wait to litle, the leader may not be ready, this is a temp fix
            // ideally call send request in a loop until it returns true
            sleep(Duration::from_millis(100)).await;
            let req: bindings::paxos::default::paxos_types::Value = Value {
                client_id: format!("client-{}", client_id),
                client_seq: next - 1,
                command: Some(Operation::Demo),
            };
            if resource
                .call_send_request(&mut *store, handle, &leader, &req)
                .await?
            {
                println!("Resending request seq {}", next - 1);
            } else {
                println!("Failed to resend request seq {}", next - 1);
            }
        }

        for resp in replies {
            let seq = resp.client_seq;
            if seen.insert(seq) {
                if let Some(t0) = sent.remove(&seq) {
                    let dt = t0.elapsed();
                    println!(
                        "[Client {}] seq={} -> {:?} in {:?}",
                        client_id, seq, resp, dt
                    );
                    lats.push(dt);
                }
            }
        }
        sleep(Duration::from_millis(1)).await;
    }

    resource.call_close(&mut *store, handle, &leader).await?;

    let total_time = start.elapsed();
    let throughput = seen.len() as f64 / total_time.as_secs_f64();
    println!("\nPersistent throughput: {:.2} req/sec\n", throughput);
    summarize("Persistent Latencies", &lats);
    Ok((client_id, lats))
}
