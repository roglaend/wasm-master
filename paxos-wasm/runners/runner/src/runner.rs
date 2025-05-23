use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "paxos-runner-world",
    });
}
bindings::export!(MyRunner with_types_in bindings);

use bindings::exports::paxos::default::runner::{Guest, GuestRunnerResource};
use bindings::paxos::default::logger;
use bindings::paxos::default::paxos_types::{Operation, PaxosRole, Value};
use bindings::paxos::default::{
    client_server::ClientServerResource,
    network_server::NetworkServerResource,
    paxos_coordinator::PaxosCoordinatorResource,
    paxos_types::{ClientResponse, Node, RunConfig},
};

struct DemoClientHelper {
    client_id: String,
    client_seq: Cell<u64>,
    start_at: Instant,
    recieved: Cell<u64>,
}

impl DemoClientHelper {
    fn new(client_id: String, delay: Duration) -> Self {
        DemoClientHelper {
            client_id,
            client_seq: Cell::new(0),
            start_at: Instant::now() + delay,
            recieved: Cell::new(0),
        }
    }

    /// Try to send demo requests once `start_at` has passed.
    fn send_if_ready(&self, paxos: &PaxosCoordinatorResource, batch_quota: u64, max_requests: u64) {
        if !paxos.is_leader() || Instant::now() < self.start_at {
            return;
        }

        // Send requests until we have recieved `batch_quota` responses
        while self.recieved.get() < batch_quota {
            let seq = self.client_seq.get();
            if seq >= max_requests {
                break;
            }
            self.client_seq.set(seq + 1);
            let req = Value {
                command: Some(Operation::Demo),
                client_id: self.client_id.clone(),
                client_seq: seq,
            };
            let _ = paxos.submit_client_request(&req);
        }
    }
}

struct MetricsHelper {
    // short‐term batch stats
    stats_batch: usize,
    batch_count: Cell<usize>,
    batch_start: RefCell<Option<Instant>>,

    // global stats (never reset until final)
    global_count: Cell<usize>,
    global_start: RefCell<Option<Instant>>,

    // for “only once” final print
    last_response: RefCell<Option<Instant>>,
    idle_logged: AtomicBool,
}

impl MetricsHelper {
    fn track(&self, num_responses: usize) {
        let now = Instant::now();

        // On *first ever* batch (i.e. batch_start is None), set both timers
        if self.global_start.borrow().is_none() {
            *self.global_start.borrow_mut() = Some(now);
            *self.batch_start.borrow_mut() = Some(now);
            self.batch_count.set(0);
            logger::log_info("[Runner] Starting global throughput timer");
        }

        // Update counts
        let new_batch = self.batch_count.get() + num_responses;
        self.batch_count.set(new_batch);

        let total = self.global_count.get() + num_responses;
        self.global_count.set(total);

        // If we’ve hit a batch, log intermediate throughput
        if new_batch >= self.stats_batch {
            let start = self.batch_start.borrow().unwrap();
            let elapsed = now.duration_since(start);
            let th = new_batch as f64 / elapsed.as_secs_f64();
            logger::log_error(&format!(
                "[Runner] Throughput: last {} responses in {:?} → {:.2} rsp/sec",
                new_batch, elapsed, th
            ));
            // reset batch
            self.batch_count.set(0);
            *self.batch_start.borrow_mut() = Some(now);
        }
    }

    /// Print a final, one-time throughput using counts from the first response
    /// up through the *last* response (so we don’t include idle time).
    fn flush_global(&self) {
        // only once
        if self.idle_logged.swap(true, Ordering::Relaxed) {
            return;
        }

        if let Some(start) = *self.global_start.borrow() {
            let total = self.global_count.get();
            if total > 0 {
                // Use the timestamp of the *last* response
                let end = self
                    .last_response
                    .borrow()
                    .expect("track must have set last_response");
                let elapsed = end.duration_since(start);
                let th = total as f64 / elapsed.as_secs_f64();
                logger::log_error(&format!(
                    "[Runner] FINAL THROUGHPUT: {} responses over {:?} → {:.2} rsp/sec",
                    total, elapsed, th
                ));
            }
        }
    }
}

struct MyRunner;
struct MyRunnerResource {
    node: Node,
    max_messages: u64,
    config: RunConfig,

    paxos: Arc<PaxosCoordinatorResource>,
    client_svr: Arc<ClientServerResource>,
    network_svr: Arc<NetworkServerResource>,

    /// A flag for future graceful shutdown
    should_stop: std::sync::atomic::AtomicBool,

    demo_client: Option<DemoClientHelper>,
    metrics: MetricsHelper,

    last_hearbeat: Cell<Instant>,
    last_failure_check: Cell<Instant>,

    constructor_called: Cell<Instant>,
}

impl Guest for MyRunner {
    type RunnerResource = MyRunnerResource;
}

impl MyRunnerResource {
    fn process_clients(&self) {
        if !self.paxos.is_leader() {
            return;
        }
        // peek at any incoming requests
        let requests = self.client_svr.get_requests(self.max_messages);
        if !requests.is_empty() {
            if self.metrics.global_start.borrow().is_none() {
                *self.metrics.global_start.borrow_mut() = Some(Instant::now());
                logger::log_info("[Runner] Starting global throughput timer");
            }
            if self.metrics.batch_start.borrow().is_none() {
                *self.metrics.batch_start.borrow_mut() = Some(Instant::now());
            }
        }

        for req in requests {
            self.paxos.submit_client_request(&req);
        }
    }

    fn process_network(&self) {
        for msg in self.network_svr.get_messages(self.max_messages) {
            self.paxos.handle_message(&msg);
        }
    }

    fn run_paxos_loop(&self) -> Option<Vec<ClientResponse>> {
        // self.paxos.failure_service(); // TODO

        self.paxos.run_paxos_loop()
    }

    fn dispatch_responses(&self, responses: Vec<ClientResponse>) {
        if !self.paxos.is_leader() {
            return;
        }
        let now = Instant::now();
        *self.metrics.last_response.borrow_mut() = Some(now);
        self.metrics.idle_logged.store(false, Ordering::Relaxed);

        if let Some(helper) = &self.demo_client {
            helper
                .recieved
                .set(helper.recieved.get() + responses.len() as u64);
        }

        logger::log_info(&format!(
            "[Runner] sending {} client responses",
            responses.len()
        ));
        self.client_svr.send_responses(&self.node, &responses);

        self.metrics.track(responses.len());
    }

    fn send_heartbeat(&self) {
        if self.config.heartbeats {
            let now = Instant::now();
            if now.duration_since(self.last_hearbeat.get())
                >= Duration::from_millis(self.config.heartbeat_interval_ms)
            {
                self.paxos.send_heartbeat();
                self.last_hearbeat.set(now);
            }
        }
    }

    // Now handles the case of leader change and start client_server on new leader
    // could prob be done cleaner but this works for now
    fn failure_check(&self) {
        let mut new_lead = None;
        if self.config.heartbeats {
            let now = Instant::now();
            if now.duration_since(self.last_failure_check.get())
                >= Duration::from_millis(self.config.heartbeat_interval_ms * 5)
            {
                new_lead = self.paxos.failure_service();
                self.last_failure_check.set(now);
            }
            if let Some(new_lead) = new_lead {
                match self.node.role {
                    PaxosRole::Proposer | PaxosRole::Coordinator => {
                        if new_lead == self.node.node_id {
                            let host = self
                                .node
                                .address
                                .split(':')
                                .next()
                                .expect("invalid node.address, expected ip:port");
                            let addr = format!("{}:{}", host, self.config.client_server_port);
                            logger::log_error(&format!(
                                "[Runner] New leader detected, setting up client server on {}",
                                &addr,
                            ));
                            self.client_svr.setup_listener(&addr);
                        }
                    }
                    PaxosRole::Acceptor => {}
                    PaxosRole::Learner => {}
                    _ => {
                        panic!("Unknown role");
                    }
                }
            }
        }
    }
}

impl GuestRunnerResource for MyRunnerResource {
    fn new(node: Node, nodes: Vec<Node>, is_leader: bool, config: RunConfig) -> Self {
        let constructor_called = Instant::now();
        let paxos = Arc::new(PaxosCoordinatorResource::new(
            &node,
            &nodes,
            is_leader,
            config.clone(),
        ));

        let num_nodes = nodes.len() as u64;
        let max_messages = config.batch_size * num_nodes;

        let client_svr = Arc::new(ClientServerResource::new());
        {
            if is_leader {
                let host = node
                    .address
                    .split(':')
                    .next()
                    .expect("invalid node.address, expected ip:port");
                let addr = format!("{}:{}", host, config.client_server_port);
                client_svr.setup_listener(&addr);
            }
        }

        let network_svr = Arc::new(NetworkServerResource::new());
        network_svr.setup_listener(&node.address);

        let demo_client = if config.demo_client {
            Some(DemoClientHelper::new(
                format!("client-{}", node.node_id),
                Duration::from_secs(3),
            ))
        } else {
            None
        };

        // sleep(Duration::from_secs(1));

        let metrics = MetricsHelper {
            stats_batch: 1000,
            batch_count: Cell::new(0),
            batch_start: RefCell::new(None),
            global_count: Cell::new(0),
            global_start: RefCell::new(None),
            last_response: RefCell::new(None),
            idle_logged: AtomicBool::new(false),
        };

        Self {
            node,
            max_messages,
            config,
            paxos,
            client_svr,
            network_svr,
            should_stop: AtomicBool::new(false),
            demo_client,
            metrics,
            last_hearbeat: Cell::new(Instant::now()),
            last_failure_check: Cell::new(Instant::now()),
            constructor_called: Cell::new(constructor_called),
        }
    }

    fn run(&self) {
        println!(
            "[Runner] Time from constructor called to running mainloop: {:?}",
            {
                let now = Instant::now();
                now.duration_since(self.constructor_called.get())
            }
        );

        let tick_duration = Duration::from_micros(self.config.tick_micros);
        let mut next_tick = Instant::now() + tick_duration;

        while !self.should_stop.load(Ordering::Relaxed) {
            // only used if configured
            self.send_heartbeat();
            self.failure_check();

            self.process_clients();
            self.process_network();

            let now = Instant::now();
            if now >= next_tick {
                if let Some(helper) = &self.demo_client {
                    helper.send_if_ready(
                        &self.paxos,
                        self.config.batch_size,
                        self.config.demo_client_requests,
                    );
                }

                if let Some(responses) = self.run_paxos_loop() {
                    self.dispatch_responses(responses);
                }
                next_tick += tick_duration;
            }

            // **Idle‐timeout check**: 1 s since last_response, print final throughput once
            // only if we've ever had a response:
            if let Some(last) = *self.metrics.last_response.borrow() {
                let idle = now.duration_since(last);
                if idle >= Duration::from_secs(1)
                    && !self.metrics.idle_logged.load(Ordering::Relaxed)
                {
                    self.metrics.flush_global();
                } else if let Some(helper) = &self.demo_client {
                    if helper.recieved.get() >= self.config.demo_client_requests {
                        // demo client is done, so we can stop
                        // used to remove the unrealistic TP from the measurements that came from no active requests just receiving
                        // should stop stops the loop, so logs only visible in log file after finish
                        self.should_stop.store(true, Ordering::Relaxed);
                        self.metrics.flush_global();
                    }
                }
            }
            // sleep "only" until our next tick
            let sleep_for = next_tick.saturating_duration_since(Instant::now());
            if !sleep_for.is_zero() {
                sleep(sleep_for);
            }
        }
    }
}
