use proto::paxos_proto::{self, network_message::Payload};
use std::time::Duration;
use tonic;

use std::{
    sync::Arc,
    time::SystemTime,
};

use tokio::time::sleep;
use tonic::Status;

use tonic::transport::Channel;

use crate::paxos_wasm::PaxosWasmtime;

#[derive(Clone)]
pub struct RunPaxosService {
    pub paxos_wasmtime: Arc<PaxosWasmtime>,
}

impl RunPaxosService {
    pub fn start_paxos_run_loop(self: Arc<Self>, interval: Duration, addr: String) {
        let _sleep_time = interval;
        tokio::spawn(async move {
            loop {
                sleep(interval).await;
                let mut store = self.paxos_wasmtime.store.lock().await;
                let resource = self.paxos_wasmtime.resource();

                // Calls the paxos coordinator to check for stale nodes
                let _ = resource
                    .call_paxos_run_loop(&mut *store, self.paxos_wasmtime.resource_handle, &addr)
                    .await
                    .map_err(|e| Status::internal(format!("Deliver message failed: {:?}", e)));
            }
        });

    }

}