use std::sync::Mutex as StdMutex;
use std::time::Instant;
use tokio::sync::Mutex as TokioMutex;

use crate::bindings::paxos::default::host_control::HostCmd;

/// Internal ping metrics, all protected by a standard (blocking) Mutex.
struct PingState {
    first_ping: Option<Instant>,
    last_ping_time: Option<Instant>,
    ping_count: u64,
}

/// Holds exactly one queued HostCmd (e.g. Shutdown), plus simple “ping” metrics.
/// Notice that both the command slot and the ping state are behind Mutexes.
/// That way, `record_ping()` can take `&self` (not `&mut self`).
pub struct HostControlHandler {
    /// At most one pending command for the Runner to pick up.
    command_slot: TokioMutex<Option<HostCmd>>,

    /// Everything below is a little internal struct that tracks pings;
    /// we wrap it in a `std::sync::Mutex` so that `record_ping(&self)` can lock it.
    ping_state: StdMutex<PingState>,
}

impl HostControlHandler {
    /// Create a new handler. Initially, no commands are queued and no pings seen.
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

    /// Record that we got a ping “just now.” Updates first_ping, last_ping_time, and ping_count.
    /// Can be called from `&self` even if `self` is wrapped in an Arc<…>.
    pub async fn record_ping(&self) {
        let now = Instant::now();

        let mut inner = self.ping_state.lock().expect("ping_inner mutex poisoned");
        if inner.first_ping.is_none() {
            inner.first_ping = Some(now);
        }
        inner.ping_count += 1;
        inner.last_ping_time = Some(now);

        // Every 10_000 pings → calculate approximate pings/sec and log it
        if inner.ping_count % 10_000 == 0 {
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

    /// Return the timestamp of the last ping, if any.
    /// Can be called from `&self`.
    pub async fn last_ping_time(&self) -> Option<Instant> {
        let inner = self.ping_state.lock().expect("ping_inner mutex poisoned");
        inner.last_ping_time
    }

    /// Called by the Runner to ask “does the Host have any command queued?”
    /// If so, pop and return it. Otherwise return None.
    pub async fn get_command(&self) -> Option<HostCmd> {
        let mut guard = self.command_slot.lock().await;
        guard.take()
    }

    /// Called by host‐side code to enqueue a `Shutdown` for the Runner.
    /// Next time `get_command()` is called, the Runner will see `Some(HostCmd::Shutdown)`.
    pub async fn request_shutdown(&self) {
        let mut guard = self.command_slot.lock().await;
        *guard = Some(HostCmd::Shutdown);
    }
}
