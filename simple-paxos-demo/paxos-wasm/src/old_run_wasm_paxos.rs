use log::{info, warn};
use wasmtime::component::*;
use wasmtime::*;

pub fn run_wasm_paxos() -> Result<(), Box<dyn std::error::Error>> {
    // Create the Wasmtime engine and store
    info!("Initializing Wasmtime engine...");
    let engine = Engine::default();
    let mut store = Store::new(&engine, ());

    // Load the components
    info!("Loading WASM components...");
    let proposer_component = Component::from_file(&engine, "proposer.component.wasm")?;
    let acceptor_component = Component::from_file(&engine, "acceptor.component.wasm")?;
    let learner_component = Component::from_file(&engine, "learner.component.wasm")?;

    // Create a Component Linker
    let mut linker = component::Linker::new(&engine);

    // Define imports for each component (if necessary)
    // linker.root().func_wrap("log", |_: (), message: String| {
    //     info!("WASM Log: {}", message);
    //     Ok(())
    // })?;

    // Instantiate the components
    info!("Instantiating components...");
    let proposer_instance = linker.instantiate(&mut store, &proposer_component)?;
    let acceptor_instance1 = linker.instantiate(&mut store, &acceptor_component)?;
    let acceptor_instance2 = linker.instantiate(&mut store, &acceptor_component)?;
    let acceptor_instance3 = linker.instantiate(&mut store, &acceptor_component)?;
    let learner_instance = linker.instantiate(&mut store, &learner_component)?;

    // Retrieve the exported functions
    info!("Retrieving exported functions...");
    let proposer_prepare =
        proposer_instance.get_typed_func::<(u64, String), (bool,)>(&mut store, "prepare")?;
    let proposer_accept =
        proposer_instance.get_typed_func::<(u64, String), (bool,)>(&mut store, "accept")?;

    let acceptor_prepare1 =
        acceptor_instance1.get_typed_func::<(u64,), (bool,)>(&mut store, "prepare")?;
    let acceptor_accept1 =
        acceptor_instance1.get_typed_func::<(u64, String), (bool,)>(&mut store, "accept")?;

    let acceptor_prepare2 =
        acceptor_instance2.get_typed_func::<(u64,), (bool,)>(&mut store, "prepare")?;
    let acceptor_accept2 =
        acceptor_instance2.get_typed_func::<(u64, String), (bool,)>(&mut store, "accept")?;

    let acceptor_prepare3 =
        acceptor_instance3.get_typed_func::<(u64,), (bool,)>(&mut store, "prepare")?;
    let acceptor_accept3 =
        acceptor_instance3.get_typed_func::<(u64, String), (bool,)>(&mut store, "accept")?;

    let learner_learn =
        learner_instance.get_typed_func::<(u64, String), ()>(&mut store, "learn")?;

    // Simulate the Paxos proposal process
    info!("Starting Paxos proposal process...");
    let proposal_id = 1;
    let value = "Value 1".to_string();

    // Step 1: Prepare phase
    info!("Executing prepare phase...");
    let prepared1 = acceptor_prepare1.call(&mut store, (proposal_id,))?;
    let prepared2 = acceptor_prepare2.call(&mut store, (proposal_id,))?;
    let prepared3 = acceptor_prepare3.call(&mut store, (proposal_id,))?;

    info!(
        "Acceptor prepare results: {}, {}, {}",
        prepared1.0, prepared2.0, prepared3.0
    );

    // Check for quorum
    let prepare_quorum = [prepared1, prepared2, prepared3]
        .iter()
        .filter(|&&result| result.0)
        .count()
        > 1;

    if prepare_quorum {
        // Step 2: Accept phase
        info!("Executing accept phase...");
        let accepted1 = acceptor_accept1.call(&mut store, (proposal_id, value.clone()))?;
        let accepted2 = acceptor_accept2.call(&mut store, (proposal_id, value.clone()))?;
        let accepted3 = acceptor_accept3.call(&mut store, (proposal_id, value.clone()))?;

        info!(
            "Acceptor accept results: {}, {}, {}",
            accepted1.0, accepted2.0, accepted3.0
        );

        // Check for quorum
        let accept_quorum = [accepted1, accepted2, accepted3]
            .iter()
            .filter(|&&result| result.0)
            .count()
            > 1;

        if accept_quorum {
            // Step 3: Inform learner
            info!(
                "Quorum achieved for value '{}'. Informing learner...",
                value
            );
            learner_learn.call(&mut store, (proposal_id, value.clone()))?;
            info!("Learner learned value: '{}'", value);
        } else {
            warn!("Quorum not achieved for accept phase.");
        }
    } else {
        warn!("Quorum not achieved for prepare phase.");
    }

    info!("Paxos process completed.");
    Ok(())
}
