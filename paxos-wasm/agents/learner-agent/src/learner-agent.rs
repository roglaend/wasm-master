use std::cell::{Ref, RefCell};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "learner-agent-world",
        // generate_unused_types: true,
        additional_derives: [PartialEq, Clone],
    });
}

bindings::export!(MyLearnerAgent with_types_in bindings);

use bindings::exports::paxos::default::learner_agent::{
    Guest as GuestLearnerAgent, GuestLearnerAgentResource,
};

use bindings::paxos::default::kv_store::KvStoreResource;
use bindings::paxos::default::learner::{LearnResult, LearnerResource};
use bindings::paxos::default::learner_types::{LearnerState, RetryLearnResult};
use bindings::paxos::default::network_types::{Heartbeat, MessagePayload, NetworkMessage};
use bindings::paxos::default::paxos_types::{
    CmdResult, ExecuteResult, Executed, KvPair, Learn, Node, PaxosRole, RunConfig, Slot, Value,
};
use bindings::paxos::default::{logger, network};

struct MyLearnerAgent;

impl GuestLearnerAgent for MyLearnerAgent {
    type LearnerAgentResource = MyLearnerAgentResource;
}

struct MyLearnerAgentResource {
    config: RunConfig,

    node: Node,
    proposers: Vec<Node>,
    acceptors: Vec<Node>,
    learner: Arc<LearnerResource>,
    kv_store: Arc<KvStoreResource>,

    /// for gap/timeout retries
    last_learn_time: RefCell<Instant>,
    retries: RefCell<HashMap<Slot, Instant>>,

    /// for executions
    exec_buffer: RefCell<VecDeque<ExecuteResult>>,
    last_exec_time: RefCell<Instant>,
}

impl MyLearnerAgentResource {
    fn adu(&self) -> Slot {
        self.get_adu()
    }

    /// Pop any newly-ready learns out of the Paxos core,
    /// apply them to the state machine, and buffer their ExecuteResults.
    fn execute_learns(&self) {
        if let LearnResult::Execute(entries) = self.learner.to_be_executed() {
            let mut buf = self.exec_buffer.borrow_mut();
            for le in entries {
                let cmd_res = self.kv_store.apply(&le.value.command); //* State Machine */
                buf.push_back(ExecuteResult {
                    slot: le.slot,
                    value: le.value.clone(),
                    success: !matches!(cmd_res, CmdResult::NoOp),
                    cmd_result: cmd_res,
                });
            }
        }
    }

    /// Collect executed results from the buffer.
    fn collect_exec_batch(&self, batch_size: Option<u64>, require_full_batch: bool) -> Executed {
        let mut buf = self.exec_buffer.borrow_mut();
        // Either up to batch size or all.
        let available = match batch_size {
            Some(n) => buf.len().min(n as usize),
            None => buf.len(),
        };

        // If we require a full batch (and there is a batch_size), but don't have enough, return
        if require_full_batch && batch_size.map_or(false, |n| available < n as usize) {
            return Executed {
                results: Vec::new(),
                adu: self.adu(),
            };
        }

        // Collect exactly the available entries
        let results: Vec<_> = buf.drain(..available).collect();
        Executed {
            results,
            adu: self.adu(),
        }
    }

    /// Dispatch any pending Executed batch to the proposers or the caller
    fn dispatch_executed(&self, executed: Executed) -> Option<NetworkMessage> {
        if executed.results.is_empty() || !self.config.learners_send_executed {
            return None;
        }

        let msg = NetworkMessage {
            sender: self.node.clone(),
            payload: MessagePayload::Executed(executed.clone()),
        };

        if self.config.is_event_driven {
            network::send_message_forget(&self.proposers, &msg);
            None
        } else {
            Some(msg)
        }
    }
}

impl GuestLearnerAgentResource for MyLearnerAgentResource {
    fn new(node: Node, nodes: Vec<Node>, config: RunConfig) -> Self {
        let proposers: Vec<_> = nodes
            .iter()
            .filter(|x| matches!(x.role, PaxosRole::Proposer | PaxosRole::Coordinator))
            .cloned()
            .collect();

        let acceptors: Vec<_> = nodes
            .clone()
            .into_iter()
            .filter(|x| matches!(x.role, PaxosRole::Acceptor | PaxosRole::Coordinator))
            .collect();

        let self_is_coordinator = node.role == PaxosRole::Coordinator;
        let num_acceptors = acceptors.len() as u64 + self_is_coordinator as u64;

        let learner = Arc::new(LearnerResource::new(num_acceptors));
        let kv_store = Arc::new(KvStoreResource::new());

        logger::log_info("[Learner Agent] Initialized.");
        Self {
            config,
            node,
            proposers,
            acceptors,
            learner,
            kv_store,
            last_learn_time: RefCell::new(Instant::now()),
            retries: RefCell::new(HashMap::new()),
            exec_buffer: RefCell::new(VecDeque::new()),
            last_exec_time: RefCell::new(Instant::now()),
        }
    }

    fn get_state(&self) -> (LearnerState, Vec<KvPair>) {
        let learner_state = self.learner.get_state();
        let kv_store_state = self.kv_store.get_state();

        (learner_state, kv_store_state)
    }

    fn get_adu(&self) -> Slot {
        self.learner.get_adu()
    }

    fn learn_and_execute(&self, slot: Slot, value: Value, sender: Node) -> Executed {
        let is_new_learn = if self.config.acceptors_send_learns {
            // Need to save the learn from the acceptor to check for quorum before executing.
            let learn = Learn {
                slot,
                value: value.clone(),
            };
            self.learner.handle_learn(&learn, &sender)
        } else {
            // Bypass the quorum check.
            self.learner.learn(slot, &value)
        };

        // If no new learn, return empty
        if !is_new_learn {
            return Executed {
                results: Vec::new(),
                adu: self.adu(),
            };
        }

        // Update timestamp and execute ready learns
        self.last_learn_time.replace(Instant::now());
        self.execute_learns();

        // TODO: Lol
        if self.config.learners_send_executed {
            // Try to collect a full batch
            let batch = self.config.executed_batch_size;
            let exec = self.collect_exec_batch(Some(batch), true);

            if exec.results.is_empty() {
                let buffered = self.exec_buffer.borrow().len();
                logger::log_info(&format!(
                    "[Learner Agent] Learned slot {}, buffered {}/{} for execution",
                    slot, buffered, batch
                ));
            } else {
                let first = exec.results.first().unwrap().slot;
                let last = exec.results.last().unwrap().slot;
                logger::log_info(&format!(
                    "[Learner Agent] Executed slots {}..{} (adu={})",
                    first,
                    last,
                    self.adu()
                ));
            }
            exec
        } else {
            Executed {
                results: vec![],
                adu: self.adu(),
            }
        }
    }

    fn execute_and_collect(&self, max_batch: Option<u64>) -> Executed {
        self.execute_learns();
        let exec = self.collect_exec_batch(max_batch, false);

        let buffered = self.exec_buffer.borrow().len();
        if exec.results.is_empty() {
            logger::log_debug(&format!(
                "[Learner Agent] No executions ready, buffered {} for execution",
                buffered
            ));
        } else {
            let first = exec.results.first().unwrap().slot;
            let last = exec.results.last().unwrap().slot;
            logger::log_info(&format!(
                "[Learner Agent] Executed slots {}..{} (adu={})",
                first,
                last,
                self.adu()
            ));
        }

        exec
    }

    /// Decide whether we need to retry learning `next`:
    /// - NoGap: neither the gap nor timeout threshold is met
    /// - Skip(next): threshold met but last retry for `next` was too recent
    /// - Retry(next): threshold met and it’s been long enough since last retry
    fn evaluate_retry(&self) -> RetryLearnResult {
        let now = Instant::now();
        let elapsed = now.duration_since(*self.last_learn_time.borrow());
        let timeout = Duration::from_millis(self.config.retry_interval_ms);

        if elapsed < timeout {
            return RetryLearnResult::NoGap;
        }

        let highest = self.learner.get_highest_learned();
        let adu = self.learner.get_adu();
        let gap = highest.saturating_sub(adu);
        let next = adu.saturating_add(1);

        if gap < self.config.learn_max_gap {
            return RetryLearnResult::NoGap;
        }

        // We *should* attempt a retry; but check whether we retried `next` too recently
        let mut retries = self.retries.borrow_mut();
        let just_retried = retries
            .get(&next)
            .map(|&last| now.duration_since(last) < timeout)
            .unwrap_or(false);

        if just_retried {
            RetryLearnResult::Skip(next)
        } else {
            // threshold met and enough time passed → record and tell caller to retry
            retries.insert(next, now);
            self.last_learn_time.replace(now);
            RetryLearnResult::Retry(next)
        }
    }

    fn run_paxos_loop(&self) {
        // 1) retry gaps/timeouts
        match self.evaluate_retry() {
            RetryLearnResult::NoGap => {
                // nothing to do
            }
            RetryLearnResult::Skip(slot) => {
                logger::log_debug(&format!("[Learner Agent] Skipping retry for slot {}", slot));
            }
            RetryLearnResult::Retry(slot) => {
                let retry_msg = NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::RetryLearn(slot),
                };
                logger::log_warn(&format!(
                    "[Learner Agent] Broadcasting RETRY LEARN for slot {}",
                    slot
                ));
                if self.config.acceptors_send_learns {
                    network::send_message_forget(&self.acceptors, &retry_msg);
                } else {
                    network::send_message_forget(&self.proposers, &retry_msg);
                }
            }
        }

        // 2) maybe execute & collect, but only every exec_interval_ms
        let now = Instant::now();
        let elapsed = now.duration_since(*self.last_exec_time.borrow());
        let interval = Duration::from_millis(self.config.exec_interval_ms);

        if elapsed < interval {
            // too soon, skip this tick
            logger::log_debug(&format!(
                "[Learner Agent] Skipping execute (only {:.0}ms since last; need {}ms)",
                elapsed.as_millis(),
                self.config.exec_interval_ms
            ));
            return;
        }

        // 3) it’s time! update timestamp and do the work
        self.last_exec_time.replace(now);
        let exec = self.execute_and_collect(Some(self.config.executed_batch_size));
        let _ = self.dispatch_executed(exec);
    }

    fn handle_message(&self, message: NetworkMessage) -> NetworkMessage {
        match message.payload {
            MessagePayload::Learn(payload) => {
                logger::log_debug(&format!(
                    "[Learner Agent] Handling LEARN: slot={}, value={:?}",
                    payload.slot, payload.value
                ));

                let executed =
                    self.learn_and_execute(payload.slot, payload.value.clone(), message.sender);

                self.dispatch_executed(executed).unwrap_or(NetworkMessage {
                    sender: self.node.clone(),
                    payload: MessagePayload::Ignore,
                })
            }

            other_message => {
                logger::log_debug(&format!(
                    "[Learner Agent] Received irrelevant message type: {:?}",
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
