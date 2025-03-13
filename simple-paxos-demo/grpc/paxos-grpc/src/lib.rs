#![allow(unsafe_op_in_unsafe_fn)]

use bindings::paxos::default::proposer::ClientProposal;
use core::panic;
use log::{info, warn};
use std::sync::Arc;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit/paxos.wit",
        world: "paxos-world",
        // async: true,
        // async: {
        //     imports: [
        //         "paxos:default/network#send-message",
        //         ],
        // TODO: This wont compile, as the compiler complains about mismatch between async and sync handles
        //     exports: [
        //         "paxos:default/paxos-coordinator#[method]paxos-coordinator-resource.prepare-phase",
        //         "paxos:default/paxos-coordinator#[method]paxos-coordinator-resource.accept-phase",
        //         "paxos:default/paxos-coordinator#[method]paxos-coordinator-resource.commit-phase",
        //         "paxos:default/paxos-coordinator#[method]paxos-coordinator-resource.run-paxos",
        //     ],
        // },
    });
}

bindings::export!(MyPaxosCoordinator with_types_in bindings);

use bindings::exports::paxos::default::paxos_coordinator::{
    AcceptedResult, ElectionResult, Guest as CoordinatorGuest, GuestPaxosCoordinatorResource,
    LearnResult, PromiseResult,
};

use bindings::paxos::default::{acceptor, kv_store, learner, network, proposer};

pub struct MyPaxosCoordinator;

impl CoordinatorGuest for MyPaxosCoordinator {
    type PaxosCoordinatorResource = MyPaxosCoordinatorResource;
}

pub struct MyPaxosCoordinatorResource {
    nodes: Vec<network::Node>,

    proposer: Arc<proposer::ProposerResource>,
    acceptor: Arc<acceptor::AcceptorResource>,
    learner: Arc<learner::LearnerResource>,
    kv_store: Arc<kv_store::KvStoreResource>,

    paxos_key: String,
}

impl MyPaxosCoordinatorResource {
    /// Returns a dynamic quorum (here, a majority).
    fn get_quorum(&self) -> u64 {
        (self.nodes.len() as u64 / 2) + 1 // TODO: Make sure is correct
    }

    /// Checks whether the given responses (from remote nodes) combined with a provided
    /// local vote count meet the required quorum. // TODO: Do this a better way?
    fn combined_quorum(&self, local_votes: u64, responses: &[network::NetworkResponse]) -> bool {
        let remote_success_count = responses
            .iter()
            .filter(|r| r.status == network::StatusKind::Success)
            .count() as u64;
        (local_votes + remote_success_count) >= self.get_quorum()
    }

    /// Determines the value to propose.
    /// According to Paxos, if any acceptor (via the promise result) has already accepted a value,
    /// that value must be proposed. Otherwise, use the client's provided value
    /// TODO: We need to store the client's values however, so that they eventually gets chosen
    fn choose_proposal_value(&self, promise: PromiseResult, client_value: &str) -> String {
        if let Some(ref accepted) = promise.accepted_value {
            info!("Using previously accepted value from promise: {}", accepted);
            accepted.clone()
        } else {
            info!(
                "No accepted value found; using client value: {}",
                client_value
            );
            client_value.to_string()
        }
    }
}

impl GuestPaxosCoordinatorResource for MyPaxosCoordinatorResource {
    // TODO: Make the num_nodes dynamic
    fn new(node_id: u64, nodes: Vec<network::Node>) -> Self {
        let num_nodes = nodes.len().clone() as u64;
        let proposer = Arc::new(proposer::ProposerResource::new(
            node_id,
            num_nodes.clone(),
            node_id == 1,
        ));
        let acceptor = Arc::new(acceptor::AcceptorResource::new());
        let learner = Arc::new(learner::LearnerResource::new());
        let kv_store = Arc::new(kv_store::KvStoreResource::new());
        Self {
            nodes,
            proposer,
            acceptor,
            learner,
            kv_store,
            paxos_key: "paxos_values".to_string(),
        }
    }

    /// Phase 1: Prepare Phase.
    ///
    /// Executes local prepare via the acceptor and sends a structured prepare message to remote nodes.
    /// Returns a PromiseResult record containing the current ballot and (if any) local acceptance info.
    fn prepare_phase(&self, ballot: u64, slot: u64) -> PromiseResult {
        // TODO: Add dedicated input argument type
        // Run local prepare via the acceptor.
        let local_prepared = self.acceptor.prepare(slot);
        if !local_prepared {
            warn!("Prepare phase: local prepare failed.");
            // In a full implementation you might abort the operation.
            panic!()
        }
        let local_votes = local_prepared as u64;

        let required_quorum = self.get_quorum();
        let local_promise = PromiseResult {
            ballot,
            accepted_ballot: 0, // TODO: Update if the local acceptor had a previously accepted proposal.
            accepted_value: None, // TODO: Update if needed.
            quorum: required_quorum,
        };

        // Build a structured prepare payload.
        let prepare_payload = network::PreparePayload { slot, ballot };
        let message = network::NetworkMessage {
            kind: network::NetworkMessageKind::Prepare,
            payload: network::MessagePayload::Prepare(prepare_payload),
        };

        // TODO: This currently uses block_on behind the scenes instead of proper async
        let responses = network::send_message(&self.nodes, &message);

        if !self.combined_quorum(local_votes, &responses) {
            warn!("Prepare phase: remote quorum check failed.");
            // Optionally handle failure here.
            panic!()
        }
        local_promise
    }

    /// Phase 2: Accept Phase.
    ///
    /// Executes local acceptance via the acceptor and sends a structured accept message to remote nodes.
    /// Returns an AcceptedResult record if quorum is reached.
    fn accept_phase(&self, proposal_value: String, slot: u64, ballot: u64) -> AcceptedResult {
        let accepted_entry = acceptor::AcceptedEntry {
            slot,
            ballot,
            value: proposal_value.clone(),
        };
        let local_accepted = self.acceptor.accept(&accepted_entry);
        if !local_accepted {
            warn!("Accept phase: local acceptance failed.");
            panic!()
        }
        let local_votes = local_accepted as u64;

        // Build a structured accept payload.
        let accept_payload = network::AcceptPayload {
            slot,
            ballot,
            proposal: proposal_value.clone(),
        };
        let message = network::NetworkMessage {
            kind: network::NetworkMessageKind::Accept,
            payload: network::MessagePayload::Accept(accept_payload),
        };

        // TODO: This currently uses block_on behind the scenes instead of proper async
        let responses = network::send_message(&self.nodes, &message);

        if !self.combined_quorum(local_votes, &responses) {
            warn!("Accept phase: combined quorum not reached.");
            panic!()
        }
        AcceptedResult {
            accepted_count: local_votes
                + responses
                    .iter()
                    .filter(|r| r.status == network::StatusKind::Success)
                    .count() as u64,
            quorum: self.get_quorum(),
        }
    }

    /// Phase 3: Commit Phase.
    ///
    /// Updates the local learner and key‑value store, then broadcasts a structured commit message.
    fn commit_phase(&self, slot: u64) -> Option<LearnResult> {
        let state = self.proposer.get_state(); // TODO: Have a cleaner way to do this, instead of getting entire state
        if let Some(prop) = state.last_proposal {
            self.learner.learn(prop.slot, &prop.client_proposal.value);
            // TODO: Maybe check if a value already exists for the slot and save all learned values instead of only latest one?
            self.kv_store
                .set(&self.paxos_key, &prop.client_proposal.value);
            let commit_payload = network::CommitPayload {
                slot,
                value: prop.client_proposal.value.clone(),
            };
            let message = network::NetworkMessage {
                kind: network::NetworkMessageKind::Commit,
                payload: network::MessagePayload::Commit(commit_payload),
            };

            // TODO: This currently uses block_on behind the scenes instead of proper async
            let _ = network::send_message(&self.nodes, &message);

            Some(LearnResult {
                learned_value: prop.client_proposal.value,
                quorum: self.get_quorum(),
            })
        } else {
            None
        }
    }

    // TODO: Async?
    fn deliver_message(&self, message: String) -> bool {
        info!("Coordinator: Received network message: {}", message);
        // In a full implementation, parse the message and update internal state.
        println!("Paxos grpc delivering message: {}", message);
        true
    }

    fn get_learned_value(&self) -> Option<String> {
        let state = self.learner.get_state();
        state.learned.first().map(|entry| entry.value.clone())
    }

    /// Orchestrates the entire Paxos protocol.
    fn run_paxos(&self, client_value: String) -> bool {
        let client_proposal = ClientProposal {
            value: client_value,
        };
        let pr: proposer::ProposeResult = self.proposer.propose(&client_proposal); // TODO: Improve function names
        if !pr.accepted {
            warn!("Proposer rejected the proposal (likely not leader).");
            return false;
        }
        let proposal = pr.proposal;

        let ballot = proposal.ballot; //* Related to who is leader
        let slot = proposal.slot; //* Placeholder for value

        // Phase 1: Prepare Phase.
        let promise = self.prepare_phase(ballot, slot);

        // TODO: Move this into a helper "propose_phase"?
        // Use the dedicated function to choose the proposal value.
        let proposal_value = self.choose_proposal_value(promise, &client_proposal.value);

        // proposal.client_proposal.value = proposal_value; // TODO: Just overwrite for now. Needs to consider not losing state and retrying values.

        // Phase 2: Accept Phase.
        let _: AcceptedResult = self.accept_phase(proposal_value, slot, ballot);
        // TODO: Currently panics if not ok, continue otherwise
        // if ar.accepted_count {
        //     warn!("Accept phase failed.");
        //     return false;
        // }

        // Phase 3: Commit Phase.
        let learn_result = self.commit_phase(slot);
        if learn_result.unwrap().learned_value.is_empty() {
            warn!("Commit phase failed.");
            return false;
        }
        true
    }

    fn elect_leader(&self) -> ElectionResult {
        todo!()
    }
}
