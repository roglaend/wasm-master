#[allow(warnings)]
mod bindings;

use bindings::exports::paxos::proposer::propose::Guest;

use bindings::paxos::acceptor::funcs::handleaccept;
use bindings::paxos::acceptor::funcs::handleprepare;

use std::collections::HashMap;
use rand::Rng;

struct Component;

const ACCEPTOR_IDS: [u32; 3] = [1, 2, 3];  

impl Guest for Component {
    fn propose(value: String) -> String {
        let proposal_number = rand::thread_rng().gen_range(1..1000);
        
        let mut promises = 0;
        let mut accepted_values = HashMap::new();

        println!("Propser got value: {value} -> Sending prepare to all acceptors...");
        // Query all acceptors
        for id in ACCEPTOR_IDS {
            let (promised, accepted_value) = handleprepare(id, proposal_number);
            println!(
                "Response from acceptor {}: Promise = {}, Value = {}",
                id,
                promised,
                accepted_value.as_deref().unwrap_or("Not accepted any values")
            );
            if promised {
                promises += 1;
                if let Some(val) = accepted_value {
                    *accepted_values.entry(val.clone()).or_insert(0) += 1;
                }
            }
        }

        // If no quorum, return failure
        if promises < 2 {
            return "Consensus not reached".to_string();
        }

        // Choose highest value already accepted
        let final_value = if let Some((most_common_value, _)) = accepted_values.iter().max_by_key(|&(_, count)| count) {
            most_common_value.clone()
        } else {
            value.clone()
        };
        println!("Propser got quorum of promises -> Sending accept request to all acceptors for value {}", final_value);

        // Send accept request
        let mut accept_count = 0;
        for id in ACCEPTOR_IDS {
            if handleaccept(id, proposal_number, &final_value) {
                println!("Acceptor {} accepted", id);
                accept_count += 1;
            }
        }

        if accept_count >= 2 {
            return format!("Consensus reached: {}", final_value);
        }
        "Consensus failed".to_string()
    }
}

bindings::export!(Component with_types_in bindings);