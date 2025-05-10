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
use bindings::paxos::default::paxos_types::{Operation, Value};
use bindings::paxos::default::{
    client_server::ClientServerResource,
    network_server::NetworkServerResource,
    paxos_coordinator::PaxosCoordinatorResource,
    paxos_types::{ClientResponse, Node, RunConfig},
};

struct DemoClientHelper {
    client_id: String,
    client_seq: Cell<u64>,
    last_request: RefCell<Instant>,
    next_interval: Cell<Duration>,
}

struct MetricsHelper {
    // short‐term batch stats
    stats_batch: usize,
    batch_count: Cell<usize>,
    batch_start: RefCell<Instant>,

    // global stats (never reset until final)
    global_count: Cell<usize>,
    global_start: RefCell<Option<Instant>>,

    // for “only once” final print
    last_response: RefCell<Instant>,
    idle_logged: AtomicBool,
}

impl MetricsHelper {
    fn track(&self, num_responses: usize) {
        // update both counters
        let short = self.batch_count.get() + num_responses;
        self.batch_count.set(short);

        let total = self.global_count.get() + num_responses;
        self.global_count.set(total);

        // mid‐run batch logging
        if short >= self.stats_batch {
            let elapsed = self.batch_start.borrow().elapsed();
            let th = short as f64 / elapsed.as_secs_f64();
            logger::log_error(&format!(
                "[Runner] Throughput: last {} responses in {:?} → {:.2} rsp/sec",
                short, elapsed, th
            ));
            // reset only the *batch* stats
            self.batch_count.set(0);
            *self.batch_start.borrow_mut() = Instant::now();
        }
    }

    /// Print a final, one‐time throughput using the global counters.
    fn flush_global(&self) {
        if let Some(start) = *self.global_start.borrow() {
            let total = self.global_count.get();
            if total > 0 {
                let elapsed = start.elapsed();
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
    config: RunConfig,

    paxos: Arc<PaxosCoordinatorResource>,
    client_svr: Arc<ClientServerResource>,
    network_svr: Arc<NetworkServerResource>,

    /// A flag for future graceful shutdown
    should_stop: std::sync::atomic::AtomicBool,

    demo_client: Option<DemoClientHelper>,
    metrics: MetricsHelper,
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
        let requests = self.client_svr.get_requests();
        if !requests.is_empty() {
            if self.metrics.global_start.borrow().is_none() {
                *self.metrics.global_start.borrow_mut() = Some(Instant::now());
                logger::log_info("[Runner] Starting global throughput timer");
            }
        }

        for req in requests {
            self.paxos.submit_client_request(&req);
        }
    }

    fn process_network(&self) {
        for msg in self.network_svr.get_messages() {
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
        let num_responses = &responses.len();
        logger::log_info(&format!(
            "[Runner] sending {} client responses",
            &num_responses
        ));
        self.client_svr.send_responses(&self.node, &responses);

        self.metrics.track(responses.len());

        // mark that we just had activity
        *self.metrics.last_response.borrow_mut() = Instant::now();
        self.metrics.idle_logged.store(false, Ordering::Relaxed);
    }

    fn demo_client_work(&self) {
        let Some(ref helper) = self.demo_client else {
            return;
        };

        if !self.paxos.is_leader() {
            return;
        }

        for _ in 0..self.config.batch_size + 10 {
            let seq = helper.client_seq.get();
            helper.client_seq.set(seq + 1);

            let req = Value {
                command: Some(Operation::Demo),
                client_id: helper.client_id.clone(),
                client_seq: seq,
            };
            let _ = self.paxos.submit_client_request(&req);
        }
    }
}

impl GuestRunnerResource for MyRunnerResource {
    fn new(node: Node, nodes: Vec<Node>, is_leader: bool, config: RunConfig) -> Self {
        let paxos = Arc::new(PaxosCoordinatorResource::new(
            &node,
            &nodes,
            is_leader,
            config.clone(),
        ));

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

        sleep(Duration::from_millis(1000)); // To allow all nodes to initialize

        let now = Instant::now();

        let demo_client = if config.demo_client {
            Some(DemoClientHelper {
                client_id: format!("client-{}", node.node_id),
                client_seq: Cell::new(0),
                last_request: RefCell::new(now),
                next_interval: Cell::new(Duration::from_millis(5)),
            })
        } else {
            None
        };

        let metrics = MetricsHelper {
            stats_batch: 100,
            batch_count: Cell::new(0),
            batch_start: RefCell::new(now),
            global_count: Cell::new(0),
            global_start: RefCell::new(None),
            last_response: RefCell::new(now),
            idle_logged: AtomicBool::new(false),
        };

        Self {
            node,
            config,
            paxos,
            client_svr,
            network_svr,
            should_stop: AtomicBool::new(false),
            demo_client,
            metrics,
        }
    }

    fn run(&self) {
        let tick_ms = self.config.tick_ms;
        let tick_duration = Duration::from_millis(tick_ms);
        let mut next_tick = Instant::now() + tick_duration;

        while !self.should_stop.load(Ordering::Relaxed) {
            self.process_clients();
            self.process_network();

            let now = Instant::now();
            if now >= next_tick {
                self.demo_client_work();

                if let Some(responses) = self.run_paxos_loop() {
                    self.dispatch_responses(responses);
                }
                next_tick += tick_duration;
            }

            // **Idle‐timeout check**: 1 s since last_response, print final throughput once
            let idle = now.duration_since(*self.metrics.last_response.borrow());
            if idle >= Duration::from_secs(1) && !self.metrics.idle_logged.load(Ordering::Relaxed) {
                self.metrics.flush_global();
                self.metrics.idle_logged.store(true, Ordering::Relaxed);
            }

            // sleep "only" until our next tick
            let sleep_for = next_tick.saturating_duration_since(Instant::now());
            if !sleep_for.is_zero() {
                sleep(sleep_for);
            }
        }
    }
}
