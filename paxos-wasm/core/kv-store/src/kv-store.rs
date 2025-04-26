use std::cell::RefCell;
use std::collections::HashMap;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "kv-store-world",
    });
}

bindings::export!(MyKvStore with_types_in bindings);

use bindings::exports::paxos::default::kv_store::{Guest, GuestKvStoreResource};
use bindings::paxos::default::logger;
use bindings::paxos::default::paxos_types::{Cmd, CmdResult, KvKey, KvPair, KvValue, Operation};

/// The concrete type for our WASM component.
pub struct MyKvStore;

impl Guest for MyKvStore {
    type KvStoreResource = MyKvStoreResource;
}

/// The key/value resource that maintains both current state and an operation history.
pub struct MyKvStoreResource {
    store: RefCell<HashMap<KvKey, KvValue>>,
    history: RefCell<Vec<Cmd>>,
}

impl MyKvStoreResource {
    /// Helper to log a command
    fn log_cmd(&self, cmd: Cmd) {
        self.history.borrow_mut().push(cmd);
    }

    /// Handle a Get operation.
    fn apply_get(&self, key: KvKey) -> Option<KvValue> {
        let val = self.store.borrow().get(&key).cloned();
        logger::log_info(&format!("[KV-Store] Get key '{}' -> {:?}", key, val));
        val
    }

    /// Handle a Set operation.
    fn apply_set(&self, kv: KvPair) -> Option<KvValue> {
        let KvPair { key, value } = kv;
        self.store.borrow_mut().insert(key.clone(), value.clone());
        logger::log_info(&format!("[KV-Store] Set key '{}' to '{}'", key, value));
        None
    }

    /// Handle a Remove operation.
    fn apply_remove(&self, key: KvKey) -> Option<KvValue> {
        let removed = self.store.borrow_mut().remove(&key);
        logger::log_info(&format!("[KV-Store] Remove key '{}' -> {:?}", key, removed));
        removed
    }

    /// Handle a Clear operation.
    fn apply_clear(&self) -> Option<KvValue> {
        self.store.borrow_mut().clear();
        logger::log_info("[KV-Store] Cleared all keys");
        None
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

    fn apply(&self, cmd: Cmd) -> CmdResult {
        self.log_cmd(cmd.clone());

        let result = match cmd {
            Cmd::None => None,
            Cmd::Some(Operation::Get(key)) => self.apply_get(key),
            Cmd::Some(Operation::Set(kv)) => self.apply_set(kv),
            Cmd::Some(Operation::Remove(key)) => self.apply_remove(key),
            Cmd::Some(Operation::Clear) => self.apply_clear(),
            Cmd::Some(Operation::Demo) => Some("Demo".to_string()),
        };

        CmdResult::from(result)
    }

    /// Get the current state as a list of key/value pairs.
    fn get_state(&self) -> Vec<KvPair> {
        self.store
            .borrow()
            .iter()
            .map(|(key, value)| KvPair {
                key: key.clone(),
                value: value.clone(),
            })
            .collect()
    }

    /// Full history of applied commands.
    fn get_history(&self) -> Vec<Cmd> {
        self.history.borrow().clone()
    }
}
