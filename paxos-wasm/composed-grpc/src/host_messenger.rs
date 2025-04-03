use crate::grpc_service::create_clients;
use crate::paxos_bindings::paxos::default::network_types::NetworkMessage;
use futures::future::join_all;
use proto::paxos_proto;
use std::time::Duration;
use tonic;
use tracing::warn;

/// A struct dedicated to dispatching network messages.
pub struct HostMessenger;

impl HostMessenger {
    /// Sends a proto network message to the given endpoints and returns the aggregated responses.
    pub async fn send_message(
        endpoints: Vec<String>,
        proto_msg: paxos_proto::NetworkMessage,
    ) -> Vec<NetworkMessage> {
        // Create gRPC clients.
        let clients_arc = create_clients(endpoints.clone()).await;
        let clients: Vec<_> = {
            let guard = clients_arc.lock();
            guard.unwrap().clone()
        };

        // Dispatch the message concurrently.
        let requests = clients.into_iter().map(|mut client| {
            let req = tonic::Request::new(proto_msg.clone());
            async move { client.deliver_message(req).await }
        });

        // TODO: Find a better way to get responses. For ex. dynamic num nodes, might have quorum before etc.
        let responses_result =
            tokio::time::timeout(Duration::from_secs(5), join_all(requests)).await;

        // Process and filter responses. // TODO: Do we want to filter the failed responses out?
        responses_result
            .ok() // If timeout occurred, this becomes None.
            .into_iter() // Convert Option to iterator (0 or 1 element).
            .flat_map(|responses| {
                responses.into_iter().filter_map(|res| match res {
                    Ok(r) => {
                        let proto_resp = r.into_inner();
                        match NetworkMessage::try_from(proto_resp) {
                            Ok(internal_resp) => Some(internal_resp),
                            Err(e) => {
                                warn!("[Host Messenger] Failed to convert proto response: {}", e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        warn!("[Host Messenger] gRPC call failed: {:?}", e);
                        None
                    }
                })
            })
            .collect()
    }
}
