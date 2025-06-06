use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::debug;

use crate::acceptor::paxos_wasm::PaxosWasmtime;

#[derive(Clone)]
pub struct RunPaxosService {
    pub paxos_wasmtime: Arc<PaxosWasmtime>,
}

impl RunPaxosService {
    pub fn start_paxos_run_loop(self: Arc<Self>, interval: Duration) {
        tokio::spawn(async move {
            loop {
                sleep(interval).await;
                let mut store = self.paxos_wasmtime.store.lock().await;
                let resource = self.paxos_wasmtime.resource();
                debug!("[Run-Paxos Service] Calling wasm component to run a new paxos loop.");
                // Calls into the the paxos run loop at an interval.
                // let _ = resource
                //     .call_handle_tick(&mut *store, self.paxos_wasmtime.resource_handle)
                //     .await
                //     .map_err(|e| Status::internal(format!("[Run-Paxos Service] Failed to call wasm component to run a new paxos loop: {:?}",
                //             e)));
            }
        });
    }
}
