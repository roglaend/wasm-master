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
use rand::Rng;

struct DemoClientHelper {
    client_id: String,
    client_seq: Cell<u64>,
    last_request: RefCell<Instant>,
    next_interval: Cell<Duration>,
}

struct MetricsHelper {
    stats_batch: usize,
    batch_count: Cell<usize>,
    batch_start: RefCell<Instant>,
}

impl MetricsHelper {
    fn track(&self, num_responses: usize) {
        let new_total = self.batch_count.get() + num_responses;
        self.batch_count.set(new_total);

        if new_total >= self.stats_batch {
            let elapsed = self.batch_start.borrow().elapsed();
            let throughput = new_total as f64 / elapsed.as_secs_f64();

            logger::log_info(&format!(
                "[Runner] last {} responses in {:?} → {:.2} rsp/sec",
                new_total, elapsed, throughput
            ));

            self.batch_count.set(0);
            *self.batch_start.borrow_mut() = Instant::now();
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
        for req in self.client_svr.get_requests() {
            self.paxos.submit_client_request(&req);
        }
    }

    fn process_network(&self) {
        for msg in self.network_svr.get_messages() {
            self.paxos.handle_message(&msg);
        }
    }

    fn tick_paxos(&self) -> Option<Vec<ClientResponse>> {
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
    }

    fn demo_client_work(&self, tick_ms: u64) {
        let Some(ref helper) = self.demo_client else {
            return;
        };

        if !self.paxos.is_leader() {
            return;
        }

        if helper.last_request.borrow().elapsed() >= helper.next_interval.get() {
            let seq = helper.client_seq.get();
            helper.client_seq.set(seq + 1);

            let req = Value {
                command: Some(Operation::Demo),
                client_id: helper.client_id.clone(),
                client_seq: seq,
            };
            let _ = self.paxos.submit_client_request(&req);
            *helper.last_request.borrow_mut() = Instant::now();

            // Randomize next interval
            let ms = rand::rng().random_range(tick_ms..(2 * tick_ms));
            helper.next_interval.set(Duration::from_millis(ms));
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

        let demo_client = if config.demo_client {
            Some(DemoClientHelper {
                client_id: format!("client-{}", node.node_id),
                client_seq: Cell::new(0),
                last_request: RefCell::new(Instant::now()),
                next_interval: Cell::new(Duration::from_millis(5)),
            })
        } else {
            None
        };

        let metrics = MetricsHelper {
            stats_batch: 100,
            batch_count: Cell::new(0),
            batch_start: RefCell::new(Instant::now()),
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
        let tick_ms = 5; // TODO: Add this to RunConfig
        let tick_duration = Duration::from_millis(tick_ms);
        let mut next_tick = Instant::now() + tick_duration;

        while !self.should_stop.load(Ordering::Relaxed) {
            self.process_clients();
            self.process_network();

            let now = Instant::now();
            if now >= next_tick {
                self.demo_client_work(tick_ms);

                if let Some(responses) = self.tick_paxos() {
                    self.dispatch_responses(responses);
                }
                next_tick += tick_duration;
            }

            // sleep "only" until our next tick
            let sleep_for = next_tick.saturating_duration_since(Instant::now());
            if !sleep_for.is_zero() {
                sleep(sleep_for);
            }
        }
    }
}
