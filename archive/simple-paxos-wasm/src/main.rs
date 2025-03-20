mod wasm_runtime;

use crate::wasm_runtime::PaxosWasmRuntime;
use anyhow::Result;
use log::warn;
use wasmtime::Engine;

const NUM_ACCEPTORS: usize = 3;

fn main() -> Result<()> {
    env_logger::init();
    println!("Running Paxos using WebAssembly components...");

    let engine = Engine::default();
    let mut wasm_runtime = PaxosWasmRuntime::new(&engine)?;

    // Create a proposer instance and get the resource handle
    let proposer_handle = wasm_runtime.create_proposer_instance(NUM_ACCEPTORS as u64)?;

    let values_to_propose = vec![
        "Value 1".to_string(),
        "Value 2".to_string(),
        "Value 3".to_string(),
    ];

    for (i, value) in values_to_propose.iter().enumerate() {
        println!("Proposing value: '{}'", value);

        let proposal_id = i as u64; // This should ideally increment over multiple proposals
        let success = wasm_runtime.propose(proposer_handle, proposal_id, value.clone())?;

        if success {
            println!("Proposal '{}' was accepted by a majority!", value);
        } else {
            warn!("Proposal '{}' failed to reach consensus.", value);
            continue;
        }
    }

    let state = wasm_runtime.get_state(proposer_handle).unwrap();

    // Print the state properly
    println!(
        "State is: proposal_id={}, last_value={}, num_acceptors={}",
        state.proposal_id,
        state.last_value.as_deref().unwrap_or("None"), // Handle Option<String>
        state.num_acceptors
    );

    println!("Run completed without errors!");
    Ok(())
}
