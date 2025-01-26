wit_bindgen::generate!({
    // the name of the world in the `*.wit` input file
    world: "kv-database",
});

struct KVStore;

impl Guest for KVStore {
    fn replace_value(key: String, value: String) -> Option<String> {
        let kv = wasm_examples::kv_store::kvdb::Connection::new();
        // replace
        let old = kv.get(&key);
        kv.set(&key, &value);
        return old;
    }
}

export!(KVStore);
