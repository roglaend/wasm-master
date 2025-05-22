use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "storage-world",
        additional_derives: [
        PartialEq,
        serde::Deserialize,
        serde::Serialize,
    ],
    });
}

bindings::export!(MyStorage with_types_in bindings);

use bindings::exports::paxos::default::storage::Guest;

// TODO: Change format from json to a binary one.
// TODO: Can also create a helper consumer of Storage in pure rust that Cores can import and use to reduce boilerplate.

struct MyStorage;

fn path_for(key: &str, suffix: &str, timestamp: Option<&str>) -> String {
    if let Some(ts) = timestamp {
        return format!("state/{}/snapshots/{}_{}", key, ts, suffix);
    }
    format!("state/{}/changes/{}", key, suffix)
}

fn load_changes_as_json(key: &str) -> Result<Vec<String>, String> {
    let changes_path = path_for(key, "changelog.jsonl", None);

    match fs::read_to_string(&changes_path) {
        Ok(content) => Ok(content.lines().map(|l| l.to_string()).collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]), // empty log
        Err(e) => Err(format!("Failed to read '{}': {}", &changes_path, e)),
    }
}

fn load_all_snapshots_as_json(key: &str) -> Result<Vec<String>, String> {
    let snapshot_dir = format!("state/{}/snapshots", key);

    let entries = match fs::read_dir(&snapshot_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]), // No snapshots yet
        Err(e) => return Err(format!("Failed to read snapshot dir: {}", e)),
    };

    let mut snapshots = Vec::new();

    for entry in entries {
        let path: PathBuf = entry
            .map_err(|e| format!("Failed to read snapshot entry: {}", e))?
            .path();

        if path.is_file() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read file {:?}: {}", path, e))?;
            snapshots.push(content);
        }
    }

    snapshots.sort();

    Ok(snapshots)
}

fn read_latest_snapshot(key: &str) -> Result<Vec<String>, String> {
    let snapshot_dir = format!("state/{}/snapshots", key);

    let entries = match fs::read_dir(&snapshot_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]), // No snapshots yet
        Err(e) => return Err(format!("Failed to read snapshot dir: {}", e)),
    };

    let mut snapshot_paths: Vec<PathBuf> = Vec::new();

    for entry in entries {
        let path = entry
            .map_err(|e| format!("Failed to read snapshot entry: {}", e))?
            .path();

        if path.is_file() {
            if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                if filename.contains("snapshot.json") {
                    snapshot_paths.push(path);
                }
            }
        }
    }

    // Sort paths by filename lexicographically (timestamp order)
    snapshot_paths.sort_by(|a, b| a.file_name().unwrap().cmp(b.file_name().unwrap()));

    // Read only the latest snapshot if any exist
    if let Some(latest_path) = snapshot_paths.last() {
        let content = fs::read_to_string(latest_path)
            .map_err(|e| format!("Failed to read file {:?}: {}", latest_path, e))?;
        Ok(vec![content])
    } else {
        Ok(vec![])
    }
}

impl Guest for MyStorage {
    fn save_state_segment(
        key: String,
        state_json: String,
        time_stamp: String,
    ) -> Result<(), String> {
        let state_path = path_for(&key, &"snapshot.json", Some(&time_stamp));
        let changes_path = path_for(&key, "changelog.jsonl", None);

        if let Some(parent) = Path::new(&state_path).parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directories for '{}': {}", state_path, e))?;
        }

        let mut file = std::fs::File::create(&state_path)
            .map_err(|e| format!("Failed to create file '{}': {}", state_path, e))?;

        file.write_all(state_json.as_bytes())
            .map_err(|e| format!("Failed to write to '{}': {}", state_path, e))?;

        fs::File::create(&changes_path)
            .map_err(|e| format!("Failed to truncate change log '{}': {}", changes_path, e))?;

        Ok(())
    }

    fn save_change(key: String, change_json: String) -> Result<(), String> {
        let path = path_for(&key, "changelog.jsonl", None);

        if let Some(parent) = Path::new(&path).parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directories for '{}': {}", path, e))?;
        }

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|e| format!("Failed to open file '{}': {}", path, e))?;

        let line = format!("{}\n", change_json);
        file.write_all(line.as_bytes())
            .map_err(|e| format!("Failed to write to '{}': {}", path, e))?;
        Ok(())
    }

    fn load_state_and_changes(key: String) -> Result<(Vec<String>, Vec<String>), String> {
        // Maybe need to thik about if this is valid, since a reloaded node will only
        // have latest snapshot, which only containts N + changes amount of state
        // should be totally fine
        let state_snapshots = read_latest_snapshot(&key)?;
        let state_changes = load_changes_as_json(&key)?;
        Ok((state_snapshots, state_changes))
    }

    fn save_state(key: String, state_json: String) -> Result<(), String> {
        let path = format!("state/{}/snapshots/{}", key, "snapshot.json");

        if let Some(parent) = Path::new(&path).parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directories for '{}': {}", path, e))?;
        }

        let mut file = std::fs::File::create(&path)
            .map_err(|e| format!("Failed to create file '{}': {}", path, e))?;

        file.write_all(state_json.as_bytes())
            .map_err(|e| format!("Failed to write to '{}': {}", path, e))?;
        Ok(())
    }

    fn load_state(key: String) -> Result<String, String> {
        let path = format!("state/{}/snapshots/{}", key, "snapshot.json");
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;
        Ok(content)
    }

    fn delete(key: String) -> bool {
        todo!()
    }

    // fn save(key: String, value: Vec<u8>) -> bool {
    //     let path = format!("state/{}", key);
    //     match fs::File::create(&path) {
    //         Ok(mut file) => {
    //             if let Err(e) = file.write_all(&value) {
    //                 eprintln!("Failed to write to file: {}", e);
    //                 return false;
    //             }
    //             true
    //         }
    //         Err(e) => {
    //             eprintln!("Failed to create file: {}", e);
    //             false
    //         }
    //     }
    // }

    // fn save_json(key: String, value: String) -> bool {
    //     let path = format!("state/{}", key);
    //     match fs::File::create(&path) {
    //         Ok(mut file) => {
    //             if let Err(e) = file.write_all(&value.as_bytes()) {
    //                 eprintln!("Failed to write to file: {}", e);
    //                 return false;
    //             }
    //             true
    //         }
    //         Err(e) => {
    //             eprintln!("Failed to create file: {}", e);
    //             false
    //         }
    //     }
    // }

    // fn load(key: String) -> Option<Vec<u8>> {
    //     match fs::read(&key) {
    //         Ok(content) => Some(content),
    //         Err(_) => None,
    //     }
    // }
}
