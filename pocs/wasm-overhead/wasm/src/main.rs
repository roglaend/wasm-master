#![allow(dead_code)]

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../../paxos-wasm/shared/wit",
        world: "wasm-overhead-world",
    });
}

use bindings::paxos::default::network_types::NetworkMessage;
use bindings::paxos::default::paxos_types::Node;
use bindings::paxos::default::{
    network_types::{Benchmark, MessagePayload},
    paxos_types,
};

use bindings::paxos::default::{network_client, network_server};

use std::env;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_MESSAGES: u64 = 100;

fn current_time_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64
}

fn dummy_payload() -> String {
    "HELLO WORLD".to_string()
}

fn run_latency_test() {
    const MESSAGES_PER_BATCH: usize = 10;
    const BATCHES: usize = 100;
    const INTERVAL_BETWEEN_BATCHES_MS: u64 = 100;

    {
        let mut server = Some(network_server::NetworkServerResource::new());
        let mut client = Some(network_client::NetworkClientResource::new());

        if let Some(s) = server.as_mut() {
            s.setup_listener("127.0.0.1:9000");
        }

        // let mut client = network_client::NetworkClientResource::new();

        let node = Node {
            node_id: 1,
            address: "127.0.0.1:9001".to_string(), // address of Node B
            role: paxos_types::PaxosRole::Proposer,
        };

        let mut rtt_samples = vec![];

        for batch in 0..BATCHES {
            // 1. Send 10 messages
            let mut sent_timestamps = vec![];

            for _ in 0..MESSAGES_PER_BATCH {
                let now = current_time_micros();

                let pay = Benchmark {
                    send_timestamp: now,
                    payload: dummy_payload(),
                };

                let msg = NetworkMessage {
                    sender: node.clone(),
                    payload: MessagePayload::Benchmark(pay),
                };

                sent_timestamps.push(now);
                if let Some(c) = client.as_mut() {
                    c.send_message_forget(&vec![node.clone()], &msg);
                }
            }

            let mut received = 0;
            let start_wait = std::time::Instant::now();
            while start_wait.elapsed() < Duration::from_millis(50) {
                if let Some(s) = server.as_mut() {
                    let messages = s.get_messages(MAX_MESSAGES);
                    for msg in messages {
                        match msg.payload {
                            MessagePayload::Benchmark(pay) => {
                                let rtt = current_time_micros() - pay.send_timestamp;
                                rtt_samples.push(rtt);
                                received += 1;
                            }
                            _ => {}
                        }
                    }
                }
                sleep(Duration::from_millis(1));
            }

            println!("[Batch {}] Received {} responses", batch + 1, received);
            sleep(Duration::from_millis(INTERVAL_BETWEEN_BATCHES_MS));
        }

        println!("\n=== RTT Results ===");
        for (i, rtt) in rtt_samples.iter().enumerate() {
            println!("Message {}: RTT = {} μs", i + 1, rtt);
        }

        let avg = rtt_samples.iter().sum::<u64>() as f64 / rtt_samples.len() as f64;
        println!("Average RTT: {:.2} μs", avg);

        drop(client.take());
        drop(server.take());
    }
}

pub fn run_throughput_test() {
    use std::time::Instant;

    const TEST_DURATION_SECS: u64 = 5;
    const MAX_REQUESTS: usize = 1000;

    let mut server = Some(network_server::NetworkServerResource::new());
    let mut client = Some(network_client::NetworkClientResource::new());

    server.as_mut().unwrap().setup_listener("127.0.0.1:9000");

    let node = Node {
        node_id: 1,
        address: "127.0.0.1:9001".to_string(), // responder address
        role: paxos_types::PaxosRole::Proposer,
    };

    let mut sent = 0;
    let mut received = 0;
    let start = Instant::now();
    let deadline = start + Duration::from_secs(TEST_DURATION_SECS);
    // Send all messages
    while Instant::now() < deadline {
        if sent < MAX_REQUESTS {
            let msg = NetworkMessage {
                sender: node.clone(),
                payload: MessagePayload::Benchmark(Benchmark {
                    send_timestamp: 0, // unused here
                    payload: dummy_payload(),
                }),
            };
            client
                .as_mut()
                .unwrap()
                .send_message_forget(&vec![node.clone()], &msg);
            sent += 1;
        }

        let messages = server.as_mut().unwrap().get_messages(MAX_MESSAGES);
        for msg in messages {
            if let MessagePayload::Benchmark(_) = msg.payload {
                received += 1;
            }
        }
        if received >= sent {
            break;
        }
    }

    // Wait for all responses (or timeout)
    let drain_end = Instant::now() + Duration::from_millis(1000);
    while Instant::now() < drain_end && received < sent {
        let messages = server.as_mut().unwrap().get_messages(MAX_MESSAGES);
        for msg in messages {
            if let MessagePayload::Benchmark(_) = msg.payload {
                received += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    let duration = Instant::now().duration_since(start).as_secs_f64();
    let throughput = received as f64 / duration;

    println!("\n=== Time-Based Throughput Test Results ===");
    println!("Duration:  {:.2} seconds", duration);
    println!("Sent:      {}", sent);
    println!("Received:  {}", received);
    println!("Throughput: {:.2} messages/sec", throughput);

    // Still gets resource has children error
    drop(client.take());
    drop(server.take());
}

pub fn run_latency_rtt_isolated() {
    const TOTAL_MESSAGES: usize = 1000;
    const RESPONSE_TIMEOUT_MS: u64 = 100;

    let mut server = Some(network_server::NetworkServerResource::new());
    let mut client = Some(network_client::NetworkClientResource::new());

    server.as_mut().unwrap().setup_listener("127.0.0.1:9000");

    let node = Node {
        node_id: 1,
        address: "127.0.0.1:9001".to_string(), // address of echo responder
        role: paxos_types::PaxosRole::Proposer,
    };

    let mut rtt_samples = vec![];

    for i in 0..TOTAL_MESSAGES {
        let msg = NetworkMessage {
            sender: node.clone(),
            payload: MessagePayload::Benchmark(Benchmark {
                send_timestamp: 0, // unused
                payload: dummy_payload(),
            }),
        };

        let start = std::time::Instant::now();
        client
            .as_mut()
            .unwrap()
            .send_message_forget(&vec![node.clone()], &msg);

        // Wait for the echo
        let timeout = std::time::Instant::now() + Duration::from_millis(RESPONSE_TIMEOUT_MS);
        loop {
            let messages = server.as_mut().unwrap().get_messages(MAX_MESSAGES);
            if let Some(_) = messages
                .into_iter()
                .find(|m| matches!(m.payload, MessagePayload::Benchmark(_)))
            {
                let rtt = start.elapsed().as_micros();
                rtt_samples.push(rtt);
                println!("Message {}: RTT = {} μs", i + 1, rtt);
                break;
            }

            if std::time::Instant::now() >= timeout {
                println!("Message {}: Timed out", i + 1);
                break;
            }

            std::thread::sleep(Duration::from_micros(100));
        }
    }

    let avg = rtt_samples.iter().sum::<u128>() as f64 / rtt_samples.len().max(1) as f64;
    println!("\n=== Isolated RTT Results ===");
    println!("Samples: {}", rtt_samples.len());
    println!("Average RTT: {:.2} μs", avg);

    drop(client.take());
    drop(server.take());
}

pub fn run_echo_responder() {
    let server = network_server::NetworkServerResource::new();
    server.setup_listener("127.0.0.1:9001"); // Responder listens here

    let client = network_client::NetworkClientResource::new();

    let target = Node {
        node_id: 0,
        address: "127.0.0.1:9000".to_string(), // Echo back to client
        role: paxos_types::PaxosRole::Proposer,
    };

    println!("[Responder] Listening on 127.0.0.1:9001");

    loop {
        let messages = server.get_messages(MAX_MESSAGES);
        for msg in messages {
            if let MessagePayload::Benchmark(benchmark_msg) = msg.payload {
                // let _ = fib(35); // Simulate some work
                let response = NetworkMessage {
                    sender: target.clone(), // This field is ignored by client
                    payload: MessagePayload::Benchmark(benchmark_msg), // Echo unchanged
                };

                client.send_message_forget(&vec![target.clone()], &response);
            }
        }

        sleep(Duration::from_millis(1)); // Prevent busy loop
    }
}

fn fib(n: u64) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        _ => fib(n - 1) + fib(n - 2),
    }
}

fn main() -> () {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <mode>", args[0]);
        eprintln!("  0 = run_echo_responder");
        eprintln!("  1 = run_latency_test");
        eprintln!("  2 = run_throughput_test");
        eprintln!("  3 = run_latency_rtt_isolated");
        std::process::exit(1);
    }

    match args[1].as_str() {
        "0" => run_echo_responder(),
        "1" => run_latency_test(),
        "2" => run_throughput_test(),
        "3" => run_latency_rtt_isolated(),
        _ => {
            eprintln!(
                "Invalid mode '{}'. Use 0 = latency, 1 = responder, 2 = throughput.",
                args[1]
            );
            std::process::exit(1);
        }
    }
}
