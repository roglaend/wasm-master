use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "learner-world",
    additional_derives: [PartialEq, Clone, Eq, Hash],
    });
}

bindings::export!(MyLearner with_types_in bindings);

use bindings::paxos::default::learner_types::LearnResult;
use bindings::paxos::default::paxos_types::{Learn, Node, Slot, Value};

use bindings::exports::paxos::default::learner::{
    Guest, GuestLearnerResource, LearnedEntry, LearnerState,
};
use bindings::paxos::default::logger;

struct MyLearner;

impl Guest for MyLearner {
    type LearnerResource = MyLearnerResource;
}

struct MyLearnerResource {
    num_acceptors: u64,

    slot_learns: RefCell<BTreeMap<Slot, HashMap<u64, Learn>>>, // TODO: Should be moved to learner agent to have consistent design
    learned: RefCell<BTreeMap<Slot, Value>>,
    executed: RefCell<BTreeMap<Slot, Value>>,
    adu: Cell<Slot>,
}

impl MyLearnerResource {
    fn quorum(&self) -> usize {
        return ((self.num_acceptors / 2) + 1) as usize;
    }

    /// Gather learns for `slot`, see if any value hits quorum.
    /// If so, returns `Some(value)`; otherwise `None`.
    fn check_quorum(&self, slot: Slot) -> Option<Value> {
        let binding = self.slot_learns.borrow();
        let slot_map = match binding.get(&slot) {
            Some(m) if m.len() >= self.quorum() => m,
            _ => return None,
        };

        // Count frequencies
        let mut counts: HashMap<&Value, usize> = HashMap::new();
        for learn in slot_map.values() {
            *counts.entry(&learn.value).or_default() += 1;
        }
        // Find any value with >= quorum
        counts
            .into_iter()
            .find(|(_, cnt)| *cnt >= self.quorum())
            .map(|(val, _)| val.clone())
    }
}

impl GuestLearnerResource for MyLearnerResource {
    fn new(num_acceptors: u64) -> Self {
        Self {
            num_acceptors,
            slot_learns: RefCell::new(BTreeMap::new()),
            learned: RefCell::new(BTreeMap::new()),
            executed: RefCell::new(BTreeMap::new()),
            adu: Cell::new(0),
        }
    }

    fn get_state(&self) -> LearnerState {
        let list = self
            .learned
            .borrow()
            .iter()
            .map(|(&slot, v)| LearnedEntry {
                slot,
                value: v.clone(),
            })
            .collect();
        LearnerState { learned: list }
    }

    fn get_adu(&self) -> Slot {
        self.adu.get()
    }

    fn get_highest_learned(&self) -> Slot {
        self.learned
            .borrow()
            .keys()
            .last()
            .copied()
            .unwrap_or_default()
    }

    /// Try to record a learned value.  
    /// Returns `true` if we actually inserted, `false` if we already had it.
    fn learn(&self, slot: Slot, value: Value) -> bool {
        let mut learned_map = self.learned.borrow_mut();
        let exec_log = self.executed.borrow();

        if learned_map.contains_key(&slot) || exec_log.contains_key(&slot) {
            logger::log_debug(&format!(
                "[Core Learner]: Slot {} already learned, ignoring {:?}",
                slot, value
            ));
            false
        } else {
            logger::log_info(&format!(
                "[Core Learner] Recording learned value {:?} for slot {}",
                value, slot
            ));
            learned_map.insert(slot, value.clone());
            true
        }
    }

    // Handles incoming learns from acceptors. Checks for quorum and and potentially stores the learned value.
    fn handle_learn(&self, learn: Learn, sender: Node) -> bool {
        // track the incoming Learn
        let slot = learn.slot;
        self.slot_learns
            .borrow_mut()
            .entry(slot)
            .or_default()
            .insert(sender.node_id, learn.clone());

        logger::log_info(&format!(
            "[Core Learner] Recorded vote for slot {}: current value {:?} (awaiting quorum).",
            slot, learn.value
        ));

        // now see if quorum is reached
        if let Some(quorum_value) = self.check_quorum(slot) {
            self.learn(slot, quorum_value)
        } else {
            false
        }
    }

    /// Try to pop up to `max_batch` learned entries, in slot order, starting from adu + 1
    fn to_be_executed(&self) -> LearnResult {
        let mut out = Vec::new();
        let mut learned = self.learned.borrow_mut();
        let mut executed = self.executed.borrow_mut();
        let mut slot = self.adu.get() + 1;

        // Pop all contiguous slots
        while let Some(val) = learned.remove(&slot) {
            executed.insert(slot, val.clone());
            out.push(LearnedEntry { slot, value: val });
            slot += 1;
        }

        // If nothing was ready, return
        if out.is_empty() {
            return LearnResult::Ignore;
        }

        // Advance our cursor
        let new_adu = slot.saturating_sub(1); // TODO: Lol
        self.adu.set(new_adu);

        let first = out.first().unwrap().slot;
        let last = out.last().unwrap().slot;
        logger::log_info(&format!(
            "[Core Learner] executing slots {}..{} ({} entries), with new adu: {}",
            first,
            last,
            out.len(),
            new_adu,
        ));
        LearnResult::Execute(out)
    }

    /// Returns the learned entry for a specific slot, if it exists.
    fn get_learned(&self, slot: u64) -> Option<LearnedEntry> {
        self.learned.borrow().get(&slot).map(|value| LearnedEntry {
            slot,
            value: value.clone(),
        })
    }
}
