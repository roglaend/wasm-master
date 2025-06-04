use std::sync::Mutex as StdMutex;
use std::time::Instant;
use tokio::sync::Mutex as TokioMutex;

use crate::bindings::paxos::default::host_control::HostCmd;

struct PingState {
    first_ping: Option<Instant>,
    last_ping_time: Option<Instant>,
    ping_count: u64,
}

pub struct HostControlHandler {
    command_slot: TokioMutex<Option<HostCmd>>,
    ping_state: StdMutex<PingState>,
}

impl HostControlHandler {
    pub fn new() -> Self {
        HostControlHandler {
            command_slot: TokioMutex::new(None),
            ping_state: StdMutex::new(PingState {
                first_ping: None,
                last_ping_time: None,
                ping_count: 0,
            }),
        }
    }

    // Record a ping now
    pub async fn record_ping(&self) {
        let now = Instant::now();
        let mut inner = self.ping_state.lock().unwrap();

        if inner.first_ping.is_none() {
            inner.first_ping = Some(now);
        }
        inner.ping_count += 1;
        inner.last_ping_time = Some(now);

        if inner.ping_count % 100_000 == 0 {
            if let Some(first) = inner.first_ping {
                let elapsed = now.duration_since(first);
                let secs = elapsed.as_secs_f64().max(1e-9);
                let rate = (inner.ping_count as f64) / secs;
                tracing::warn!(
                    "[Host Control] {} pings in {:.3?} → {:.2} ping/sec",
                    inner.ping_count,
                    elapsed,
                    rate
                );
            }
        }
    }

    // Get time of last ping, if any
    pub async fn last_ping_time(&self) -> Option<Instant> {
        let inner = self.ping_state.lock().unwrap();
        inner.last_ping_time
    }

    // Runner calls to fetch a pending command, if one exists
    pub async fn get_command(&self) -> Option<HostCmd> {
        let mut guard = self.command_slot.lock().await;
        guard.take()
    }

    // Host calls to request shutdown for the Runner
    pub async fn request_shutdown(&self) {
        let mut guard = self.command_slot.lock().await;
        *guard = Some(HostCmd::Shutdown);
    }
}
