use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "acceptor-world",
        additional_derives: [PartialEq],
    });
}

bindings::export!(MyAcceptor with_types_in bindings);

use bindings::exports::paxos::default::acceptor::{Guest, GuestAcceptorResource};
use bindings::paxos::default::acceptor_types::{AcceptedResult, AcceptorState, PromiseResult};
use bindings::paxos::default::logger;
use bindings::paxos::default::paxos_types::{Accepted, Ballot, PValue, Promise, Slot, Value};

pub struct MyAcceptor;

impl Guest for MyAcceptor {
    type AcceptorResource = MyAcceptorResource;
}

pub struct MyAcceptorResource {
    // A list of all promised ballots. // TODO: Only care about latest, but still nice to keep track of.
    promises: RefCell<Vec<Ballot>>,
    // Map from slot to the accepted proposal.
    accepted: RefCell<BTreeMap<Slot, PValue>>,

    // The garbage collection window: number of recent slots to retain.
    gc_window: u64,
    // Interval of performing GC.
    gc_interval: u64,
    // The slot number when GC was last performed.
    last_gc: Cell<Slot>,
}

impl MyAcceptorResource {
    /// Returns the current promised ballot (or 0 if no promise has been made yet).
    fn highest_promised_ballot(&self) -> Ballot {
        self.promises.borrow().last().cloned().unwrap_or(0)
    }

    /// Helper to collect accepted proposals for slots >= the given slot.
    fn collect_accepted_from(&self, slot: Slot) -> Vec<PValue> {
        self.accepted
            .borrow()
            .range(slot..) // returns all key-value pairs with key >= slot
            .map(|(_, p)| p.clone())
            .collect()
    }

    /// Returns the highest slot present in the accepted proposals map (or 0 if none).
    fn _highest_accepted_slot(&self) -> Slot {
        self.accepted
            .borrow()
            .keys()
            .next_back()
            .cloned()
            .unwrap_or(0)
    }
}

impl GuestAcceptorResource for MyAcceptorResource {
    fn new(gc_window: Option<u64>) -> Self {
        Self {
            promises: RefCell::new(Vec::new()),
            accepted: RefCell::new(BTreeMap::new()),

            gc_window: gc_window.unwrap_or(100), // use provided gc_window or default to 100
            gc_interval: 10, // default to 10 for now, maybe pass it down from main config as with gc_window
            last_gc: Cell::new(0),
        }
    }

    /// Processes an prepare request for a proposal.
    fn prepare(&self, slot: Slot, ballot: Ballot) -> PromiseResult {
        let highest_ballot = self.highest_promised_ballot();

        if ballot < highest_ballot {
            logger::log_warn(&format!(
                "[Core Acceptor] Slot {} rejected prepare with ballot {} (current highest promised ballot: {})",
                slot, ballot, highest_ballot
            ));
            return PromiseResult::Rejected(highest_ballot);
        }

        // Only update the promise history if the new ballot is strictly higher.
        if ballot > highest_ballot {
            self.promises.borrow_mut().push(ballot);
            logger::log_info(&format!(
                "[Core Acceptor] Added a new promise for ballot {} (was {})",
                ballot, highest_ballot
            ));
        } else {
            logger::log_info(&format!(
                "[Core Acceptor] Received idempotent prepare for slot {} with ballot {}", // TODO: Ignore this case?
                slot, ballot
            ));
        }

        // Collect all accepted proposals (PValue) with slot >= the input slot.
        let accepted = self.collect_accepted_from(slot);
        logger::log_info(&format!(
            "[Core Acceptor] Collected {} accepted proposals for slots >= {}",
            accepted.len(),
            slot
        ));

        let promise = Promise {
            slot,
            ballot,
            accepted,
        };

        self.auto_garbage_collect(slot);
        PromiseResult::Promised(promise)
    }

    /// Processes an accept request for a proposal.
    fn accept(&self, slot: Slot, ballot: Ballot, value: Value) -> AcceptedResult {
        let current_promised = self.highest_promised_ballot();

        if ballot != current_promised {
            logger::log_warn(&format!(
                "[Core Acceptor] Rejected accept for slot {} with ballot {} (current highest promised ballot: {})",
                slot, ballot, current_promised
            ));
            return AcceptedResult::Rejected(current_promised);
        }

        let mut accepted_map = self.accepted.borrow_mut();
        if let Some(existing) = accepted_map.get(&slot) {
            // If the accepted proposal already exists:
            if existing.ballot == ballot && existing.value == Some(value.clone()) {
                // Idempotent case: the same value is being re-accepted.  // TODO: Might not be needed, could maybe just ignore instead.
                logger::log_info(&format!(
                    "[Core Acceptor] Re-accepted idempotently for slot {} with ballot {}",
                    slot, ballot
                ));
                return AcceptedResult::Accepted(Accepted {
                    slot,
                    ballot,
                    success: true,
                });
            } else {
                // A conflicting proposal exists for this slot; reject the new request.
                logger::log_warn(&format!(
                    "[Core Acceptor] Rejected accept for slot {} with ballot {} because a conflicting proposal already exists (existing ballot: {}, value: {:?})",
                    slot, ballot, existing.ballot, existing.value
                ));
                return AcceptedResult::Rejected(existing.ballot);
            }
        } else {
            // No proposal has been accepted for this slot yet; accept the new proposal.
            let p_value = PValue {
                slot,
                ballot,
                value: Some(value.clone()),
            };
            accepted_map.insert(slot, p_value);
            logger::log_info(&format!(
                "[Core Acceptor] Accepted proposal for slot {} with ballot {}",
                slot, ballot
            ));
            self.auto_garbage_collect(slot);
            return AcceptedResult::Accepted(Accepted {
                slot,
                ballot,
                success: true,
            });
        }
    }

    /// Returns the current state: lists of promise entries and accepted proposals.
    fn get_state(&self) -> AcceptorState {
        let promises_list = self.promises.borrow().clone();
        let accepted_list = self.accepted.borrow().values().cloned().collect();
        AcceptorState {
            promises: promises_list,
            accepted: accepted_list,
        }
    }
}

impl MyAcceptorResource {
    fn auto_garbage_collect(&self, current_slot: Slot) {
        let next_gc_slot = self.last_gc.get() + self.gc_interval;
        if current_slot >= next_gc_slot {
            if current_slot > self.gc_window {
                let threshold = current_slot - self.gc_window;
                // self.promises
                //     .borrow_mut()
                //     .retain(|&slot, _| slot >= threshold); // TODO: Enable if too many promises causes and issue.
                self.accepted
                    .borrow_mut()
                    .retain(|&slot, _| slot >= threshold);
                logger::log_info(&format!(
                    "[Core Acceptor] Auto garbage collected state for slots below {}",
                    threshold
                ));
            }
            // Update the last GC slot to the current slot.
            self.last_gc.set(current_slot);
        }
    }
}
