#![allow(unsafe_op_in_unsafe_fn)]

use log::info;
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

pub struct MyKvStore;

impl Guest for MyKvStore {
    type KvStoreResource = MyKvStoreResource;
}

/// The kv–store maintains an in–memory hash map for key/value storage.
pub struct MyKvStoreResource {
    store: RefCell<HashMap<String, String>>,
}

impl GuestKvStoreResource for MyKvStoreResource {
    /// Constructor: Initialize an empty key/value store.
    fn new() -> Self {
        Self {
            store: RefCell::new(HashMap::new()),
        }
    }

    /// Retrieve a value for a given key.
    fn get(&self, key: String) -> Option<String> {
        self.store.borrow().get(&key).cloned()
    }

    /// Set a key to a given value.
    fn set(&self, key: String, value: String) {
        self.store.borrow_mut().insert(key.clone(), value.clone());
        info!("KV-Store: Set key '{}' to value '{}'", key, value);
    }

    /// Remove a key from the store.
    fn remove(&self, key: String) -> Option<String> {
        let removed = self.store.borrow_mut().remove(&key);
        if let Some(ref value) = removed {
            info!("KV-Store: Removed key '{}' with value '{}'", key, value);
        }
        removed
    }

    /// Clear the store.
    fn clear(&self) {
        self.store.borrow_mut().clear();
        info!("KV-Store: Cleared all keys.");
    }
}
