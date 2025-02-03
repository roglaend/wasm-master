use std::{env, error::Error};

use paxos_rust::network::distributed::{run_client, run_node};
/// Entry point for distributed mode.  
/// Usage:
///   To start a node:   cargo run --release -- node <node_id> <address> <peer1> <peer2> ...
///   To run as client:  cargo run --release -- client <target_address> <command>
pub fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage:");
        println!(
            "  Node mode:   {} node <node_id> <address> <peer1> <peer2> ...",
            args[0]
        );
        println!(
            "  Client mode: {} client <target_address> <command>",
            args[0]
        );
        return Ok(());
    }
    match args[1].as_str() {
        "node" => {
            if args.len() < 4 {
                println!(
                    "Usage for node mode: {} node <node_id> <address> <peer1> <peer2> ...",
                    args[0]
                );
                return Ok(());
            }
            let node_id: usize = args[2].parse()?;
            let address = args[3].clone();
            let peers = if args.len() > 4 {
                args[4..].to_vec()
            } else {
                Vec::new()
            };
            run_node(node_id, &address, peers)?;
        }
        "client" => {
            if args.len() < 4 {
                println!(
                    "Usage for client mode: {} client <target_address> <command>",
                    args[0]
                );
                return Ok(());
            }
            let target_addr = args[2].clone();
            let command = args[3..].join(" ");
            run_client(&target_addr, command)?;
        }
        _ => {
            println!("Unknown mode: {}", args[1]);
        }
    }
    Ok(())
}
