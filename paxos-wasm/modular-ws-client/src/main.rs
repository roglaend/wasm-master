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

    let paxos = PaxosWasmtime::new(&engine, client_world).await.unwrap();

    match args.mode {
        ClientMode::Oneshot => {
            // ————— Oneshot —————
            let mut lats = Vec::with_capacity(args.num_requests);
            let start = Instant::now();
            for seq in 0..args.num_requests {
                let val = Value {
                    client_id: args.client_id.to_string(),
                    client_seq: seq as u64,
                    command: None,
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
            let guest = paxos.bindings.paxos_default_paxos_client();
            let resource = guest.paxos_client_resource();
            let mut store = paxos.store.lock().await;
            let handle = resource.call_constructor(&mut *store).await?;

            let total = args.num_requests as u64;
            let mut next = 0;
            let mut sent = std::collections::HashMap::new();
            let mut seen = std::collections::HashSet::new();
            let mut lats = Vec::with_capacity(args.num_requests);
            let start = Instant::now();
            let deadline = start + Duration::from_secs(args.timeout_secs);

            while (seen.len() as u64) < total && Instant::now() < deadline {
                // send
                if next < total {
                    let req = Value {
                        client_id: args.client_id.to_string(),
                        client_seq: next,
                        command: Some(Operation::Demo),
                    };
                    if resource
                        .call_send_request(&mut *store, handle, &args.leader, &req)
                        .await?
                    {
                        sent.insert(next, Instant::now());
                        println!("→ sent seq={}", next);
                        next += 1;
                    }
                }

                // receive
                let replies = resource
                    .call_try_receive(&mut *store, handle, &args.leader)
                    .await?;
                for resp in replies {
                    let seq = resp.client_seq;
                    if seen.insert(seq) {
                        let dt = sent.remove(&seq).unwrap().elapsed();
                        println!("← recv seq={} in {:?}", seq, dt);
                        lats.push(dt);
                    }
                }

                sleep(Duration::from_millis(1)).await;
            }

            resource
                .call_close(&mut *store, handle, &args.leader)
                .await?;
            let wall = start.elapsed();
            let tput = seen.len() as f64 / wall.as_secs_f64();
            println!("\nPersistent throughput: {:.2} req/sec\n", tput);
            summarize("Persistent Latencies", &lats);
        }
    }

    Ok(())
}
