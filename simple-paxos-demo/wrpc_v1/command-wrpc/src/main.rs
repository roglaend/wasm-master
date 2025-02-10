mod bindings {
    wit_bindgen::generate!({
        with: {
            "paxos:wrpc/proposer": generate,
        }
    });
}

fn main() {
    println!("COMMAND IS RUNNING");

    let proposer = bindings::paxos::wrpc::proposer::ProposerResource::new(1);

    let answer = proposer.propose(1, &"Value 1");

    println!("ANSWER FROM PROPOSER, Accepted?: {}", answer)
}
