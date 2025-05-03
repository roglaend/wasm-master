use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "learner-world",
    additional_derives: [PartialEq],
    });
}

bindings::export!(MyLearner with_types_in bindings);

use bindings::paxos::default::paxos_types::{Accepted, Learn, Node, Slot, Value};

use bindings::exports::paxos::default::learner::{
    Guest, GuestLearnerResource, LearnResult, LearnedEntry, LearnerState,
};
use bindings::paxos::default::logger;

struct MyLearner;

impl Guest for MyLearner {
    type LearnerResource = MyLearnerResource;
}

struct MyLearnerResource {
    learned: RefCell<BTreeMap<Slot, Value>>,
    next_to_execute: Cell<Slot>,
    execution_log: RefCell<BTreeMap<Slot, Value>>,
    max_gap_size: u64,
    num_acceptors: u64,

    slot_learns: RefCell<BTreeMap<Slot, HashMap<u64, Learn>>>, // TODO: new, needed?
    flush_timeout: Duration,
    last_flush: Cell<Instant>, // TODO: move to agent

    retry_timeout: Duration, // TODO: move to agent
    last_message_time: Cell<Instant>,
}

impl MyLearnerResource {
    fn quorum(&self) -> usize {
        return ((self.num_acceptors / 2) + 1) as usize;
    }
}

impl GuestLearnerResource for MyLearnerResource {
    /// Constructor: Initialize an empty BTreeMap.
    fn new(num_acceptors: u64) -> Self {
        Self {
            learned: RefCell::new(BTreeMap::new()),
            next_to_execute: Cell::new(1),
            execution_log: RefCell::new(BTreeMap::new()),
            max_gap_size: 10,
            num_acceptors,

            slot_learns: RefCell::new(BTreeMap::new()),
            flush_timeout: Duration::from_millis(10),
            last_flush: Cell::new(Instant::now()),

            retry_timeout: Duration::from_millis(500),
            last_message_time: Cell::new(Instant::now()),
        }
    }

    fn get_state(&self) -> LearnerState {
        let learned_list: Vec<LearnedEntry> = self
            .execution_log
            .borrow()
            .iter()
            .map(|(&slot, value)| LearnedEntry {
                slot,
                value: value.clone(),
            })
            .collect();
        LearnerState {
            learned: learned_list,
        }
    }

    fn get_next_to_execute(&self) -> Slot {
        self.next_to_execute.get()
    }

    /// Record that a value has been learned for a given slot.
    /// If the slot already has a learned value, a warning is logged and the new value is ignored.
    /// Can only execute consecutive slots starting from the next_to_execute slot.
    fn learn(&self, slot: Slot, value: Value) -> LearnResult {
        let mut learned_map = self.learned.borrow_mut();
        let execution_log = self.execution_log.borrow_mut();

        // Insert learn if have not learned yet
        if !learned_map.contains_key(&slot) && !execution_log.contains_key(&slot) {
            logger::log_info(&format!(
                "[Core Learner]: For slot {}, learned value {:?}",
                slot, value
            ));
            learned_map.insert(slot, value);
        } else {
            logger::log_warn(&format!(
                "Learner: Slot {} already has a learned value. Ignoring new value {:?}.",
                slot, value
            ));
        }
        return LearnResult::Ignore;
    }

    // TODO

    // Handles incoming learns from acceptors. Checks for quorum and and stores the learned value. Returns ready to be executed slots if any.
    fn handle_learn(&self, learn: Learn, from: Node) -> LearnResult {
        let now = Instant::now();
        self.last_message_time.set(now);
        let execution_log = self.execution_log.borrow();
        if !execution_log.contains_key(&learn.slot) {
            logger::log_info(&format!(
                "[Core Learner]: Received learn for slot {} from node {}",
                learn.slot, from.node_id
            ));
            let slot = learn.slot;
            self.slot_learns
                .borrow_mut()
                .entry(slot)
                .or_insert_with(HashMap::new)
                .insert(from.node_id, learn.clone());

            if let Some(sender_map) = self.slot_learns.borrow().get(&slot) {
                if sender_map.len() >= self.quorum() {
                    let learns: Vec<&Learn> = sender_map.values().collect();

                    for &candidate in &learns {
                        let count = learns.iter().filter(|&&learn| learn == candidate).count();

                        if count >= self.quorum() {
                            logger::log_info(&format!(
                                "[Core Learner]: Learned full Learn {:?} for slot {} with count {}",
                                candidate, slot, count
                            ));
                            // Learn: candidate.value or the full candidate
                            self.learned
                                .borrow_mut()
                                .insert(slot, candidate.value.clone());
                            break;
                        }
                    }
                }
            }
        }
        return LearnResult::Ignore;
    }

    // TODO

    fn to_be_executed(&self) -> LearnResult {
        let mut learned_map = self.learned.borrow_mut();
        let mut next_to_execute = self.get_next_to_execute();

        let mut contiguous_ready = 0;
        let mut probe_slot = next_to_execute;

        // First just *count* how many contiguous slots are ready
        while learned_map.contains_key(&probe_slot) {
            contiguous_ready += 1;
            probe_slot += 1;

            if contiguous_ready >= 10 {
                // Tcp socket problems if message is to big. Also noticed increadbly slowdowns when sending 20+ slots
                break;
            }
        }

        // Ensure 10 is sent at the sime time - boost throughput by 50ops/s ca. Need to itroduce some mechanism here to ensure that
        // if we do not have 10 slots ready based on some timeout we need to send the slots we have
        // or else the system will be stuck wating for more slots to be learned.
        // This is fine when we are testing with with request_size % 10 = 0
        if contiguous_ready >= 10 {
            let mut to_be_executed = Vec::new();
            let mut execution_log = self.execution_log.borrow_mut();

            // Now actually remove and execute them
            for _ in 0..contiguous_ready {
                if let Some(val) = learned_map.remove(&next_to_execute) {
                    execution_log.insert(next_to_execute, val.clone());
                    to_be_executed.push(LearnedEntry {
                        slot: next_to_execute,
                        value: val,
                    });
                    next_to_execute += 1;
                    self.next_to_execute.set(next_to_execute);
                }
            }

            return LearnResult::Execute(to_be_executed);
        } else {
            LearnResult::Ignore
        }
        // Check if some learns are ready to
    }

    // Checker for gaps in the learned slots. Should be called at reasoinable a interval.
    fn check_for_gap(&self) -> Option<Slot> {
        let learned_map = self.learned.borrow_mut();
        let next_to_execute = self.next_to_execute.get();
        let max_learned_slot = learned_map.keys().max().copied().unwrap_or(0);

        // If no slot beyond next_to_execute has been learned, nothing to gap.
        if max_learned_slot <= next_to_execute {
            return None;
        }

        // If the next slot is already learned, there is no gap.
        if learned_map.contains_key(&next_to_execute) {
            return None;
        }

        // Compute the gap between the maximum learned slot and the next expected one.
        let gap = max_learned_slot - next_to_execute;

        let now = Instant::now();
        // return if gap is > max_gap_size or if the time since last message is > retry_timeout
        if gap >= self.max_gap_size
            || (gap > 0 && now.duration_since(self.last_message_time.get()) > self.retry_timeout)
        {
            Some(next_to_execute)
        } else {
            None
        }
    }

    /// Returns the learned entry for a specific slot, if it exists.
    fn get_learned(&self, slot: u64) -> Option<LearnedEntry> {
        self.learned.borrow().get(&slot).map(|value| LearnedEntry {
            slot,
            value: value.clone(),
        })
    }
}
