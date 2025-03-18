#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::RefCell;
use std::collections::HashMap;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit/paxos.wit",
        world: "kv-store-world",
    });
}

bindings::export!(MyKvStore with_types_in bindings);

use crate::bindings::exports::paxos::default::kv_store::{Guest, GuestKvStoreResource};
use crate::bindings::paxos::default::logger;

/// The concrete type for our WASM component.
pub struct MyKvStore;

impl Guest for MyKvStore {
    type KvStoreResource = MyKvStoreResource;
}

/// Internal representation of an operation.
#[derive(Clone, Debug)]
enum Operation {
    Set { key: String, value: String },
    Remove { key: String },
    Clear,
}

/// The key/value resource that maintains both current state and an operation history.
pub struct MyKvStoreResource {
    store: RefCell<HashMap<String, String>>,
    history: RefCell<Vec<Operation>>,
}

impl MyKvStoreResource {
    /// Helper to log a "set" operation.
    fn log_set(&self, key: &str, value: &str) {
        self.history.borrow_mut().push(Operation::Set {
            key: key.to_string(),
            value: value.to_string(),
        });
    }

    /// Helper to log a "remove" operation.
    fn log_remove(&self, key: &str) {
        self.history.borrow_mut().push(Operation::Remove {
            key: key.to_string(),
        });
    }

    /// Helper to log a "clear" operation.
    fn log_clear(&self) {
        self.history.borrow_mut().push(Operation::Clear);
    }

    /// Apply a single operation to the current state.  //* Proof of concept */
    fn _apply_operation(&self, op: &Operation) {
        match op {
            Operation::Set { key, value } => {
                self.store.borrow_mut().insert(key.clone(), value.clone());
            }
            Operation::Remove { key } => {
                self.store.borrow_mut().remove(key);
            }
            Operation::Clear => {
                self.store.borrow_mut().clear();
            }
        }
    }

    /// Rebuild the current state by replaying the entire history.  //* Proof of concept */
    fn _rebuild_state(&self) {
        self.store.borrow_mut().clear();
        for op in self.history.borrow().iter() {
            self._apply_operation(op);
        }
    }
}

impl GuestKvStoreResource for MyKvStoreResource {
    /// Constructor: Initialize an empty key/value store and an empty history.
    fn new() -> Self {
        Self {
            store: RefCell::new(HashMap::new()),
            history: RefCell::new(Vec::new()),
        }
    }

    /// Retrieve a value for a given key.
    fn get(&self, key: String) -> Option<String> {
        self.store.borrow().get(&key).cloned()
    }

    /// Set a key to a given value.
    fn set(&self, key: String, value: String) {
        self.store.borrow_mut().insert(key.clone(), value.clone());
        self.log_set(&key, &value);
        logger::log_info(&format!("KV-Store: Set key '{}' to value '{}'", key, value));
    }

    /// Remove a key from the store.
    fn remove(&self, key: String) -> Option<String> {
        let removed = self.store.borrow_mut().remove(&key);
        self.log_remove(&key);
        if let Some(ref value) = removed {
            logger::log_info(&format!(
                "KV-Store: Removed key '{}' with value '{}'",
                key, value
            ));
        }
        removed
    }

    /// Clear the store.
    fn clear(&self) {
        self.store.borrow_mut().clear();
        self.log_clear();
        logger::log_info("KV-Store: Cleared all keys.");
    }

    /// Get the current state as a list of key/value pairs.
    fn get_state(&self) -> Vec<bindings::exports::paxos::default::kv_store::KvPair> {
        self.store
            .borrow()
            .iter()
            .map(
                |(key, value)| bindings::exports::paxos::default::kv_store::KvPair {
                    key: key.clone(),
                    value: value.clone(),
                },
            )
            .collect()
    }

    /// Get the full history of operations.
    fn get_history(&self) -> Vec<bindings::exports::paxos::default::kv_store::Operation> {
        self.history
            .borrow()
            .iter()
            .map(|op| match op {
                Operation::Set { key, value } => {
                    bindings::exports::paxos::default::kv_store::Operation::Set(
                        bindings::exports::paxos::default::kv_store::KvPair {
                            key: key.clone(),
                            value: value.clone(),
                        },
                    )
                }
                Operation::Remove { key } => {
                    bindings::exports::paxos::default::kv_store::Operation::Remove(key.clone())
                }
                Operation::Clear => bindings::exports::paxos::default::kv_store::Operation::Clear,
            })
            .collect()
    }
}
