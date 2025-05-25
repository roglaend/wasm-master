use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::{
    cell::RefCell,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
};

mod bindings {
    wit_bindgen::generate!({
        path: "../../shared/wit",
        world: "storage-world",
        additional_derives: [ Clone, PartialEq ],
    });
}
bindings::export!(MyStorage with_types_in bindings);

use bindings::{
    exports::paxos::default::storage::{Bytes, Guest, GuestStorageResource, StateAndChanges},
    paxos::default::logger,
};

struct MyStorage;
impl Guest for MyStorage {
    type StorageResource = MyStorageResource;
}

struct MyStorageResource {
    base_dir: String,           // e.g. "state"
    key: String,                // e.g. "node7-learner"
    snapshots_dir: String,      // base_dir/key/snapshots
    changelog_path: String,     // base_dir/key/changelog.bin
    current_state_path: String, // base_dir/key/current-state.bin
    max_snapshots: u64,
    writer: RefCell<BufWriter<File>>, // open changelog writer
    state_file: RefCell<File>,
}

impl GuestStorageResource for MyStorageResource {
    fn new(key: String, max_snapshots: u64) -> Self {
        let base = "state".to_string();
        let key_dir = format!("{}/{}", base, key);
        let snapshots_dir = format!("{}/snapshots", &key_dir);
        let changelog_path = format!("{}/changelog.bin", &key_dir);
        let current_state_path = format!("{}/current-state.bin", &key_dir);

        // ensure dirs
        fs::create_dir_all(&snapshots_dir).expect("mkdir snapshots");

        // open changelog writer
        let changelog_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&changelog_path)
            .expect("open changelog");
        let writer = RefCell::new(BufWriter::new(changelog_file));

        let state_file = RefCell::new(
            OpenOptions::new()
                .create(true)
                .write(true)
                .open(&current_state_path)
                .expect("open current-state"),
        );

        MyStorageResource {
            base_dir: base,
            key,
            snapshots_dir,
            changelog_path,
            current_state_path,
            max_snapshots,
            writer,
            state_file,
        }
    }

    /// Overwrite the "current-state" meant for keeping the latest version of a single state.
    fn save_state(&self, state: Bytes) -> Result<(), String> {
        let mut f = self.state_file.borrow_mut();
        f.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        f.set_len(0).map_err(|e| e.to_string())?;
        f.write_all(&state).map_err(|e| e.to_string())?;
        logger::log_info(&format!(
            "[Storage] save_state: wrote {} bytes",
            state.len()
        ));
        Ok(())
    }

    /// Read only the latest version of "current-state".
    fn load_state(&self) -> Result<Bytes, String> {
        let data = fs::read(&self.current_state_path).map_err(|e| e.to_string())?;
        logger::log_info(&format!(
            "[Storage] load_state: loaded {} bytes from '{}'",
            data.len(),
            self.current_state_path
        ));
        Ok(data)
    }

    /// Force‐flush the latest "current-state"
    fn flush_state(&self) -> Result<(), String> {
        let f = self.state_file.borrow_mut();
        f.sync_all().map_err(|e| e.to_string())?;
        logger::log_info("[Storage] flush_state: file fsynced");
        Ok(())
    }

    /// Append one change-record to the changelog.
    fn save_change(&self, change: Bytes) -> Result<(), String> {
        let len = change.len();
        let mut w = self.writer.borrow_mut();
        w.write_u32::<BigEndian>(len as u32)
            .and_then(|_| w.write_all(&change))
            .map_err(|e| e.to_string())?;
        logger::log_info(&format!("[Storage] save_change: buffered {} bytes", len));
        Ok(())
    }

    /// Force‐flush any buffered changes in the changelog to disk.
    fn flush_changes(&self) -> Result<(), String> {
        let mut w = self.writer.borrow_mut();
        w.flush().map_err(|e| e.to_string())?;
        logger::log_info("[Storage] flush_changes(): writer flushed");
        Ok(())
    }

    /// Atomically create/rotate a new snapshot and truncate changelog.
    fn checkpoint(&self, state: Bytes, timestamp: String) -> Result<(), String> {
        let filename = format!("snapshot-{}.bin", timestamp);
        let tmp = format!("{}/{}.tmp", self.snapshots_dir, filename);
        let final_path = format!("{}/{}", self.snapshots_dir, filename);

        // Write to tmp + fsync the file itself
        {
            let mut f = File::create(&tmp).map_err(|e| e.to_string())?;
            f.write_all(&state).map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
        }

        // Atomic rename
        fs::rename(&tmp, &final_path).map_err(|e| e.to_string())?;

        // Truncate & fsync the changelog
        {
            let mut w = self.writer.borrow_mut();
            w.flush().map_err(|e| e.to_string())?;
            let mut raw = w.get_ref();
            raw.set_len(0).map_err(|e| e.to_string())?;
            raw.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            raw.sync_all().map_err(|e| e.to_string())?;
        }

        // Prune on‐disk snapshots
        self.prune_snapshots(self.max_snapshots)?;
        Ok(())
    }

    /// Return [ snapshot-<x1>.bin, …, snapshot-<xN>.bin ] in timestamp order
    fn list_snapshots(&self) -> Vec<String> {
        // Try to read the directory; if it fails, return an empty list
        let mut files = match fs::read_dir(&self.snapshots_dir) {
            Ok(rd) => rd
                // ignore any individual entry errors
                .filter_map(|entry| entry.ok())
                .filter_map(|de| {
                    let name = de.file_name().into_string().ok()?;
                    if name.starts_with("snapshot-") && name.ends_with(".bin") {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };

        // Sort lexicographically (ISO timestamps will sort in chronological order)
        files.sort_unstable();
        files
    }

    /// delete oldest until ≤ retain
    fn prune_snapshots(&self, retain: u64) -> Result<(), String> {
        let mut snaps = self.list_snapshots();
        while snaps.len() as u64 > retain {
            let oldest = snaps.remove(0);
            let path = format!("{}/{}", self.snapshots_dir, oldest);
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Read back the latest N snapshots and the changelog
    fn load_state_and_changes(&self, num_snapshots: u64) -> Result<StateAndChanges, String> {
        // Pick the last N snapshot files
        let snaps = {
            let all = self.list_snapshots();
            let start = all.len().saturating_sub(num_snapshots as usize);
            all.into_iter()
                .skip(start)
                .map(|name| {
                    let path = format!("{}/{}", self.snapshots_dir, name);
                    fs::read(&path).map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?
        };

        // Read changelog entries
        let mut changes = Vec::new();
        if let Ok(mut f) = File::open(&self.changelog_path) {
            loop {
                match f.read_u32::<BigEndian>() {
                    Ok(len) => {
                        let mut buf = vec![0; len as usize];
                        f.read_exact(&mut buf).map_err(|e| e.to_string())?;
                        changes.push(buf);
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(e) => return Err(e.to_string()),
                }
            }
        }

        Ok((snaps, changes))
    }

    /// Delete `state/<key>/`
    fn delete(&self) -> bool {
        let dir = format!("{}/{}", self.base_dir, self.key);
        let ok = fs::remove_dir_all(&dir).is_ok();
        logger::log_info(&format!("[Storage] delete: removed '{}', ok={}", dir, ok));
        ok
    }
}
