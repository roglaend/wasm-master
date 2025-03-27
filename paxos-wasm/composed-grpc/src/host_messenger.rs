use crate::grpc_service::create_clients;
use crate::paxos_bindings;
use futures::future::join_all;
use proto::paxos_proto;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tonic;

/// A struct dedicated to dispatching network messages.
pub struct HostMessenger;

impl HostMessenger {
    pub async fn send_message_forget(
        endpoints: Vec<String>,
        proto_msg: paxos_proto::NetworkMessage,
        message_kind: paxos_bindings::paxos::default::network::NetworkMessageKind)
        -> () {
        }


    /// Sends a proto network message to the given endpoints and returns the aggregated responses.
    pub async fn send_message(
        endpoints: Vec<String>,
        proto_msg: paxos_proto::NetworkMessage,
        message_kind: paxos_bindings::paxos::default::network::NetworkMessageKind,
    ) -> Vec<paxos_bindings::paxos::default::network::NetworkResponse> {
        // Create gRPC clients.
        let responses = async {
            let clients_arc = create_clients(endpoints.clone()).await;
            let clients: Vec<_> = {
                let guard = clients_arc.lock();
                guard.unwrap().clone()
            };

            // Dispatch the message concurrently.
            let futures: Vec<_> = clients
                .into_iter()
                .map(|mut client| {
                    let req = tonic::Request::new(proto_msg.clone());
                    async move { client.deliver_message(req).await }
                })
                .collect();

            // TODO: Find a better way to get responses. For ex. dynamic num nodes, might have quorum before etc.
            tokio::time::timeout(Duration::from_secs(1), join_all(futures)).await
        }
        .await;

        // Construct network responses based on the gRPC results.
        let network_responses: Vec<_> = match responses {
            Ok(results) => results
                .into_iter()
                .map(|res| {
                    let success = res.map(|r| r.into_inner().success).unwrap_or(false);
                    let status = if success {
                        paxos_bindings::paxos::default::network::StatusKind::Success
                    } else {
                        paxos_bindings::paxos::default::network::StatusKind::Failure
                    };
                    paxos_bindings::paxos::default::network::NetworkResponse {
                        kind: message_kind,
                        status,
                    }
                })
                .collect(),
            Err(_) => endpoints
                .into_iter()
                .map(
                    |_| paxos_bindings::paxos::default::network::NetworkResponse {
                        kind: message_kind,
                        status: paxos_bindings::paxos::default::network::StatusKind::Failure,
                    },
                )
                .collect(),
        };
        network_responses
    }
}
