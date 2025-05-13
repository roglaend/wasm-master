mod bindings;
mod host_network_client;
mod host_network_server;
mod serializer;

use bindings::paxos::default::network_types::NetworkMessage;
use bindings::paxos::default::paxos_types::Node;
use bindings::paxos::default::{
    network_types::{Benchmark, MessagePayload},
    paxos_types,
};

use host_network_client::NativeTcpClient;
use host_network_server::NativeTcpServer;

use std::env;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn current_time_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64
}

fn dummy_payload() -> String {
    "HELLO WORLD".to_string()
}

fn get_listener_address(port: u16, docker: bool) -> String {
    if docker {
        format!("0.0.0.0:{}", port)
    } else {
        format!("127.0.0.1:{}", port)
    }
}

fn get_target_address(docker: bool, container_name: &str, port: u16) -> String {
    if docker {
        format!("{}:{}", container_name, port)
    } else {
        format!("127.0.0.1:{}", port)
    }
}

pub fn run_latency_test(docker: bool) {
    const MESSAGES_PER_BATCH: usize = 10;
    const BATCHES: usize = 100;
    const INTERVAL_BETWEEN_BATCHES_MS: u64 = 100;

    let mut server = NativeTcpServer::new();
    server.setup_listener(&get_listener_address(9000, docker));

    let mut client = NativeTcpClient::new();

    let node = Node {
        node_id: 1,
        address: get_target_address(docker, "node-b", 9001),
        role: paxos_types::PaxosRole::Proposer,
    };

    let mut rtt_samples = vec![];

    for batch in 0..BATCHES {
        let mut sent_timestamps = vec![];

        for _ in 0..MESSAGES_PER_BATCH {
            let now = current_time_micros();
            let msg = NetworkMessage {
                sender: node.clone(),
                payload: MessagePayload::Benchmark(Benchmark {
                    send_timestamp: now,
                    payload: dummy_payload(),
                }),
            };

            sent_timestamps.push(now);
            client.send_message_forget(vec![node.clone()], msg);
        }

        let mut received = 0;
        let start_wait = std::time::Instant::now();
        while start_wait.elapsed() < Duration::from_millis(50) {
            let messages = server.get_messages();
            for msg in messages {
                if let MessagePayload::Benchmark(pay) = msg.payload {
                    let rtt = current_time_micros() - pay.send_timestamp;
                    rtt_samples.push(rtt);
                    received += 1;
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

    let avg = rtt_samples.iter().sum::<u64>() as f64 / rtt_samples.len().max(1) as f64;
    println!("Average RTT: {:.2} μs", avg);
}

pub fn run_throughput_test(docker: bool) {
    use std::time::Instant;

    const TEST_DURATION_SECS: u64 = 5;
    const MAX_REQUESTS: usize = 1000;

    let mut server = Some(NativeTcpServer::new());
    let mut client = Some(NativeTcpClient::new());

    server
        .as_mut()
        .unwrap()
        .setup_listener(&get_listener_address(9000, docker));

    let node = Node {
        node_id: 1,
        address: get_target_address(docker, "node-b", 9001),
        role: paxos_types::PaxosRole::Proposer,
    };

    let mut sent = 0;
    let mut received = 0;
    let start = Instant::now();
    let deadline = start + Duration::from_secs(TEST_DURATION_SECS);

    while Instant::now() < deadline {
        // Send next message if under cap
        if sent < MAX_REQUESTS {
            let msg = NetworkMessage {
                sender: node.clone(),
                payload: MessagePayload::Benchmark(Benchmark {
                    send_timestamp: 0,
                    payload: dummy_payload(),
                }),
            };

            client
                .as_mut()
                .unwrap()
                .send_message_forget(vec![node.clone()], msg);

            sent += 1;
        }

        // Try to receive any responses
        let messages = server.as_mut().unwrap().get_messages();
        for msg in messages {
            if let MessagePayload::Benchmark(_) = msg.payload {
                received += 1;
            }
        }
        if received >= sent {
            break;
        }
    }

    // Final drain pass for any stragglers
    let drain_end = Instant::now() + Duration::from_millis(20000);
    while Instant::now() < drain_end {
        let messages = server.as_mut().unwrap().get_messages();
        for msg in messages {
            if let MessagePayload::Benchmark(_) = msg.payload {
                received += 1;
            }
        }
        if received >= sent {
            break;
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

    drop(client.take());
    drop(server.take());
}

pub fn run_latency_rtt_isolated(docker: bool) {
    const TOTAL_MESSAGES: usize = 1000;
    const RESPONSE_TIMEOUT_MS: u64 = 100;

    let mut server = Some(NativeTcpServer::new());
    let mut client = Some(NativeTcpClient::new());

    server
        .as_mut()
        .unwrap()
        .setup_listener(&get_listener_address(9000, docker));

    let node = Node {
        node_id: 1,
        address: get_target_address(docker, "node-b", 9001),
        role: paxos_types::PaxosRole::Proposer,
    };

    let mut rtt_samples = vec![];

    for i in 0..TOTAL_MESSAGES {
        let now = current_time_micros();
        let msg = NetworkMessage {
            sender: node.clone(),
            payload: MessagePayload::Benchmark(Benchmark {
                send_timestamp: now,
                payload: dummy_payload(),
            }),
        };

        let start = std::time::Instant::now();
        client
            .as_mut()
            .unwrap()
            .send_message_forget(vec![node.clone()], msg);

        let timeout = std::time::Instant::now() + Duration::from_millis(RESPONSE_TIMEOUT_MS);
        loop {
            let messages = server.as_mut().unwrap().get_messages();
            if messages
                .iter()
                .any(|m| matches!(m.payload, MessagePayload::Benchmark(_)))
            {
                let rtt = current_time_micros() - now;
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

    let avg = rtt_samples.iter().sum::<u64>() as f64 / rtt_samples.len().max(1) as f64;
    println!("\n=== Isolated RTT Results ===");
    println!("Samples: {}", rtt_samples.len());
    println!("Average RTT: {:.2} μs", avg);

    drop(client.take());
    drop(server.take());
}

pub fn run_echo_responder(docker: bool) {
    println!("Echo with fib");
    let mut server = NativeTcpServer::new();
    server.setup_listener(&get_listener_address(9001, docker));

    let mut client = NativeTcpClient::new();
    let target = Node {
        node_id: 0,
        address: get_target_address(docker, "node-a", 9000),
        role: paxos_types::PaxosRole::Proposer,
    };

    println!(
        "[Responder] Listening on {}",
        get_listener_address(9001, docker)
    );

    loop {
        let messages = server.get_messages();
        for msg in messages {
            // let _ = fib(35); // Simulate some work
            if let MessagePayload::Benchmark(b) = msg.payload {
                let response = NetworkMessage {
                    sender: target.clone(),
                    payload: MessagePayload::Benchmark(b.clone()),
                };
                client.send_message_forget(vec![target.clone()], response);
            }
        }
        sleep(Duration::from_millis(1));
    }
}

fn fib(n: u64) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        _ => fib(n - 1) + fib(n - 2),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <mode> [--docker]", args[0]);
        eprintln!("  0 = run_echo_responder");
        eprintln!("  1 = run_latency_test");
        eprintln!("  2 = run_throughput_test");
        eprintln!("  3 = run_latency_rtt_isolated");
        std::process::exit(1);
    }

    let docker = args.iter().any(|a| a == "--docker");

    match args[1].as_str() {
        "0" => run_echo_responder(docker),
        "1" => run_latency_test(docker),
        "2" => run_throughput_test(docker),
        "3" => run_latency_rtt_isolated(docker),
        _ => {
            eprintln!("Invalid mode '{}'", args[1]);
            std::process::exit(1);
        }
    }
}
