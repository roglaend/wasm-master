use anyhow;
use wrpc_transport::{self};

mod bindings {
    wit_bindgen_wrpc::generate!({
        with: {
            "paxos:wrpc/proposer": generate,
        }
    });
}

#[tokio::main()]
async fn main() -> anyhow::Result<()> {
    println!("COMMAND IS RUNNING");

    let addr = "[::1]:7761";

    let acceptor_addresses: &[&str] = &["321", "333"];

    let wrpc = wrpc_transport::tcp::Client::from(addr);

    let proposer_resource_handle =
        bindings::paxos::wrpc::proposer::ProposerResource::new(&wrpc, (), addr, acceptor_addresses)
            .await
            .unwrap();

    let test_value = bindings::paxos::wrpc::proposer::get_test_value(&wrpc, ())
        .await
        .unwrap();
    println!("Test Value: {:?}", test_value);

    let handle_borrow = &proposer_resource_handle.as_borrow();

    
    let proposal_id = 1;
    let proposal_value = format!("Propose Value: {}", proposal_id);

    let answer = bindings::paxos::wrpc::proposer::ProposerResource::propose(
        &wrpc,
        (),
        handle_borrow,
        proposal_id,
        &proposal_value,
    )
    .await
    .unwrap();

    println!("ANSWER FROM PROPOSER, Accepted?: {}", answer);

    let state =
        bindings::paxos::wrpc::proposer::ProposerResource::get_state(&wrpc, (), handle_borrow)
            .await
            .expect("failed to get state");

    println!("PROPOSER STATE: {:?}", state);
    Ok(())
}
