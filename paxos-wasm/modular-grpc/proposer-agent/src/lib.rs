use std::sync::Arc;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "proposer-agent-world",
        // generate_unused_types: true,
        additional_derives: [PartialEq],
    });
}

bindings::export!(MyProposerAgent with_types_in bindings);

use bindings::exports::paxos::default::proposer_agent::{
    Guest as ProposerGuest, GuestProposerAgentResource,
};
use bindings::paxos::default::network_types::{Heartbeat, MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{
    Accept, Accepted, Ballot, ClientRequest, Node, PaxosRole, Prepare, Promise, Proposal, Slot,
    Value,
};
use bindings::paxos::default::proposer_types::{AcceptResult, PrepareResult};
use bindings::paxos::default::{logger, network, proposer::ProposerResource};

pub struct MyProposerAgent;

impl ProposerGuest for MyProposerAgent {
    type ProposerAgentResource = MyProposerAgentResource;
}

pub struct MyProposerAgentResource {
    node: Node,
    acceptors: Vec<Node>,
    proposer: Arc<ProposerResource>,
}

impl GuestProposerAgentResource for MyProposerAgentResource {
    fn new(node: Node, nodes: Vec<Node>, is_leader: bool) -> Self {
        let acceptors: Vec<_> = nodes
            .into_iter()
            .filter(|x| x.role == PaxosRole::Acceptor || x.role == PaxosRole::Coordinator)
            .collect();
        // TODO: make this more future proof?
        let num_acceptors = acceptors.len() as u64
            + if node.role == PaxosRole::Coordinator {
                1
            } else {
                0
            };
        let init_ballot = node.node_id;
        let proposer = Arc::new(ProposerResource::new(is_leader, num_acceptors, init_ballot));
        logger::log_info(&format!(
            "[Proposer Agent] Initialized with node_id={} as {} leader. ({} acceptors)",
            node.node_id,
            if is_leader { "a" } else { "not a" },
            num_acceptors
        ));
        Self {
            node,
            acceptors,
            proposer,
        }
    }

    /// Submits a client request to the core proposer.
    fn submit_client_request(&self, req: ClientRequest) -> bool {
        let result = self.proposer.enqueue_client_request(&req);
        logger::log_debug(&format!(
            "[Proposer Agent] Submitted client request '{:?}': {}.",
            req, result
        ));
        result
    }

    /// Retrieves the proposal for the given slot from the core proposer, if available.
    fn get_proposal(&self, slot: u64) -> Option<Proposal> {
        self.proposer.get_proposal(slot)
    }

    /// Creates a new proposal using the core proposer.
    fn create_proposal(&self) -> Option<Proposal> {
        let proposal = self.proposer.create_proposal();
        match &proposal {
            Some(p) => logger::log_debug(&format!(
                "[Proposer Agent] Created proposal: slot={}, ballot={}, value={:?}.",
                p.slot, p.ballot, p.client_request.value
            )),
            None => logger::log_warn(
                "[Proposer Agent] Failed to create proposal (not leader or no pending requests).",
            ),
        }
        proposal
    }

    /// Executes a full Paxos instance from the proposers pov by creating a proposal and running prepare and accept phases.
    fn run_paxos(&self, req: ClientRequest) -> bool {
        self.submit_client_request(req.clone()); // TODO: Decouple the running of paxos from the submission of requests

        logger::log_info(&format!(
            "[Proposer Agent] Starting Paxos round for client value {:?}.",
            req.value
        ));

        let prop = match self.create_proposal() {
            Some(p) => p,
            None => return false,
        };

        // Prepare Phase: process promise responses.
        let prepare_outcome = match self.prepare_phase(prop.slot, prop.ballot, vec![]) {
            PrepareResult::Outcome(outcome) => outcome,
            PrepareResult::QuorumFailure => {
                logger::log_error("[Proposer Agent] Prepare phase quorum failure.");
                return false;
            }
            PrepareResult::MissingProposal => {
                logger::log_error(&format!(
                    "[Proposer Agent] Prepare phase failed due to missing in-flight proposal for slot {}.",
                    prop.slot
                ));
                return false;
            }
        };
        logger::log_info(&format!(
            "[Proposer Agent] Prepare outcome for slot {}: chosen_value={:?}, is_original={}.",
            prop.slot, prepare_outcome.chosen_value, prepare_outcome.is_original
        ));

        // Use the chosen value from prepare phase.
        let final_value = prepare_outcome.chosen_value;

        // Accept Phase: broadcast accept and process responses.
        let accept_outcome = self.accept_phase(final_value.clone(), prop.slot, prop.ballot, vec![]);
        match accept_outcome {
            AcceptResult::Accepted(_) => {
                if self.finalize_proposal(prop.slot, final_value) {
                    logger::log_info(&format!(
                        "[Proposer Agent] Paxos round for slot {} completed successfully.",
                        prop.slot
                    ));
                }
                true
            }
            AcceptResult::QuorumFailure | AcceptResult::MissingProposal => {
                logger::log_error(
                    "[Proposer Agent] Accept phase failed to reach quorum, was rejected, or proposal was missing.",
                );
                false
            }
        }
    }

    /// Broadcasts a prepare message, collects promise responses, and processes them.
    fn prepare_phase(&self, slot: u64, ballot: u64, mut promises: Vec<Promise>) -> PrepareResult {
        let prepare = Prepare { slot, ballot };
        let msg = NetworkMessage {
            sender: self.node.clone(),
            payload: MessagePayload::Prepare(prepare),
        };
        logger::log_debug(&format!(
            "[Proposer Agent] Broadcasting PREPARE: slot={}, ballot={}.",
            slot, ballot
        ));

        let responses = network::send_message(&self.acceptors, &msg);
        logger::log_debug(&format!(
            "[Proposer Agent] Received {} promise responses.",
            responses.len()
        ));

        responses
            .into_iter()
            .filter_map(|resp| {
                if let MessagePayload::Promise(prom_payload) = resp.payload {
                    Some(Promise {
                        ballot: prom_payload.ballot,
                        accepted: prom_payload.accepted,
                    })
                } else {
                    logger::log_warn("[Proposer Agent] Received successful response with unexpected payload variant.");
                    None
                }
            })
             .for_each(|pr| promises.push(pr));

        // Use the core proposer to process the promise responses.
        self.proposer.process_prepare(slot, &promises)
    }

    // TODO: Maybe use proposer-agent/coordinator specific return types here, instead of reusing from core proposer?

    /// Broadcasts an accept message, collects responses, and processes them.
    fn accept_phase(
        &self,
        value: Value,
        slot: Slot,
        ballot: Ballot,
        mut accepts: Vec<Accepted>,
    ) -> AcceptResult {
        let accept = Accept {
            slot,
            ballot,
            value: value.clone(),
        };
        let msg = NetworkMessage {
            sender: self.node.clone(),
            payload: MessagePayload::Accept(accept),
        };
        logger::log_info(&format!(
            "[Proposer Agent] Broadcasting ACCEPT: slot={}, ballot={}, value={:?}.",
            slot, ballot, value
        ));

        let responses = network::send_message(&self.acceptors, &msg);
        logger::log_debug(&format!(
            "[Proposer Agent] Received {} accepted responses.",
            responses.len()
        ));

        // Process responses by filtering for success and converting Accepted payloads.
        responses
            .into_iter()
            .filter_map(|resp| {
                if let MessagePayload::Accepted(acc_payload) = resp.payload {
                    Some(Accepted {
                        slot: acc_payload.slot,
                        ballot: acc_payload.ballot,
                        success: acc_payload.success,
                    })
                } else {
                    logger::log_warn(
                        "[Proposer Agent} Received successful response with unexpected payload variant.",
                    );
                    None
                }
            })
            .for_each(|acc| accepts.push(acc));

        // Use the core proposer to process the accepted responses.
        self.proposer.process_accept(slot, &accepts)
    }

    fn finalize_proposal(&self, slot: u64, chosen_value: Value) -> bool {
        let result = self.proposer.finalize_proposal(slot, &chosen_value);
        if result {
            logger::log_info(&format!(
                "[Proposer Agent] Finalized proposal for slot {} with chosen value {:?}.",
                slot, chosen_value
            ));
        } else {
            logger::log_error(&format!(
                "[Proposer Agent] Failed to finalize proposal for slot {} with chosen value {:?}.",
                slot, chosen_value
            ));
        }
        result
    }

    fn handle_message(&self, message: NetworkMessage) -> NetworkMessage {
        logger::log_info(&format!(
            "[Proposer Agent] Received network message: {:?}",
            message
        ));

        // TODO: Fully implement and make use of the handling of incoming Promise and Accepted, when we use fire and forget messaging

        match message.payload {
            MessagePayload::Promise(payload) => {
                logger::log_info(&format!(
                    "[Proposer Agent] Handling PROMISE: ballot={}, accepted={:?}",
                    payload.ballot, payload.accepted
                ));
                // TODO: Perform the necessary task here to keep track of all incoming promises for a slot when using "fire-and-forget"
                let _result = true;
                // TODO: Ensure to factor out logic so we can reuse it when we wait for responses and handle them synchronously.

                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore, // TODO: No message to send back anyway?
                }
            }
            MessagePayload::Accepted(payload) => {
                logger::log_info(&format!(
                    "[Proposer Agent] Handling ACCEPTED: slot={}, ballot={}, success={}",
                    payload.slot, payload.ballot, payload.success
                ));
                // TODO: Perform the necessary task here to keep track of all the incoming accepted messages when using "fire-and-forget"
                let _result = true;
                // TODO: Ensure to factor out logic so we can reuse it when we wait for responses and handle them synchronously.

                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore, // TODO: No message to send back anyway?
                }
            }
            MessagePayload::Heartbeat(payload) => {
                logger::log_info(&format!(
                    "[Proposer Agent] Handling HEARTBEAT: sender: {:?}, timestamp={}",
                    payload.sender, payload.timestamp
                ));
                // Simply echo the heartbeat payload.
                let response_payload = Heartbeat {
                    sender: self.node.clone(),
                    // timestamp = ... // TODO: Have a consistent way to define these?
                    timestamp: payload.timestamp,
                };
                // TODO: Have a dedicated heartbeat ack payload type?
                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Heartbeat(response_payload),
                }
            }
            other_message => {
                logger::log_warn(&format!(
                    "[Proposer Agent] Received irrelevant message type: {:?}",
                    other_message
                ));
                NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore,
                }
            }
        }
    }
}
