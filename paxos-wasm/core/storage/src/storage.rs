// src/storage_component.rs

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::{
    cell::RefCell,
    fs::{self, File, OpenOptions, rename},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
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
    exports::paxos::default::storage::{Guest, GuestStorageResource, StateAndChanges},
    paxos::default::logger,
};

struct MyStorage;
impl Guest for MyStorage {
    type StorageResource = MyStorageResource;
}

struct MyStorageResource {
    base_dir: String,                 // e.g. "state"
    key: String,                      // e.g. "node7-learner"
    writer: RefCell<BufWriter<File>>, // open changelog writer
}

impl GuestStorageResource for MyStorageResource {
    /// constructor(key: string)
    fn new(key: String) -> Self {
        // 1) ensure base structure once
        let base = "state".to_string();
        let snapshots_dir = format!("{}/{}/snapshots", base, key);
        let changelog_dir = format!("{}/{}/changelog.bin", base, key);

        fs::create_dir_all(&snapshots_dir).ok();
        if let Some(parent) = Path::new(&changelog_dir).parent() {
            fs::create_dir_all(parent).ok();
        }

        logger::log_info(&format!(
            "[Storage] init: base='{}', key='{}', snapshots_dir='{}', changelog_dir='{}'",
            base, key, snapshots_dir, changelog_dir
        ));

        // 2) open changelog writer once
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&changelog_dir)
            .expect("open changelog");
        let writer = RefCell::new(BufWriter::new(file));

        MyStorageResource {
            base_dir: base,
            key,
            writer,
        }
    }

    /// Atomically overwrite snapshot + truncate changelog (staged, no fsync)
    fn save_state_segment(&self, state: Vec<u8>, timestamp: String) -> Result<(), String> {
        let snapshots_dir = format!("{}/{}/snapshots", self.base_dir, self.key);
        let final_path = format!("{}/snapshot.bin", &snapshots_dir);
        let tmp_path = format!("{}.{}", &final_path, "tmp");

        logger::log_info(&format!(
            "[Storage] STEP 1: writing {} bytes to tmp='{}' (ts={})",
            state.len(),
            tmp_path,
            timestamp
        ));

        // Stage 1: write .tmp
        {
            let mut f = File::create(&tmp_path).map_err(|e| e.to_string())?;
            f.write_all(&state).map_err(|e| e.to_string())?;
            // skip f.sync_all() for performance
        }
        logger::log_info("[Storage] STEP 1 complete");

        // Host could interleave other tasks here...

        // Stage 2: rename into place
        rename(&tmp_path, &final_path).map_err(|e| e.to_string())?;
        logger::log_info(&format!("[Storage] STEP 2: renamed tmp → '{}'", final_path));

        // Stage 3: truncate the in-memory changelog writer
        {
            let mut w = self.writer.borrow_mut();
            w.flush().map_err(|e| e.to_string())?;
            let mut raw = w.get_ref();
            raw.set_len(0).map_err(|e| e.to_string())?;
            raw.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        }
        logger::log_info("[Storage] STEP 3: changelog truncated");

        Ok(())
    }

    /// Append one change-record
    fn save_change(&self, change: Vec<u8>) -> Result<(), String> {
        let len = change.len();
        {
            let mut w = self.writer.borrow_mut();
            w.write_u32::<BigEndian>(len as u32)
                .and_then(|_| w.write_all(&change))
                .map_err(|e| e.to_string())?;
            w.flush().map_err(|e| e.to_string())?;
        }
        logger::log_info(&format!(
            "[Storage] save_change: appended {} bytes to changelog",
            len
        ));
        Ok(())
    }

    /// Read back snapshot + changelog
    fn load_state_and_changes(&self) -> Result<StateAndChanges, String> {
        // Snapshot
        let mut snaps = Vec::new();
        let snap_file = format!("{}/{}/snapshots/snapshot.bin", self.base_dir, self.key);
        if Path::new(&snap_file).exists() {
            let data = fs::read(&snap_file).map_err(|e| e.to_string())?;
            snaps.push(data);
            logger::log_info(&format!("[Storage] load: found snapshot='{}'", snap_file));
        } else {
            logger::log_info(&format!("[Storage] load: no snapshot at '{}'", snap_file));
        }

        // Changelog
        let mut changes = Vec::new();
        let log_file = format!("{}/{}/changelog.bin", self.base_dir, self.key);
        if let Ok(mut f) = File::open(&log_file) {
            let mut count = 0;
            loop {
                match f.read_u32::<BigEndian>() {
                    Ok(len) => {
                        let mut buf = vec![0; len as usize];
                        f.read_exact(&mut buf).map_err(|e| e.to_string())?;
                        changes.push(buf);
                        count += 1;
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(e) => return Err(e.to_string()),
                }
            }
            logger::log_info(&format!(
                "[Storage] load: read {} changelog entries from '{}'",
                count, log_file
            ));
        } else {
            logger::log_info(&format!("[Storage] load: no changelog at '{}'", log_file));
        }

        Ok((snaps, changes))
    }

    /// Overwrite snapshot only (fast, no changelog touch)
    fn save_state(&self, state: Vec<u8>) -> Result<(), String> {
        let path = format!("{}/{}/snapshots/snapshot.bin", self.base_dir, self.key);
        File::create(&path)
            .and_then(|mut f| f.write_all(&state))
            .map_err(|e| e.to_string())?;
        logger::log_info(&format!(
            "[Storage] save_state: wrote {} bytes to '{}'",
            state.len(),
            path
        ));
        Ok(())
    }

    /// Read only latest snapshot
    fn load_state(&self) -> Result<Vec<u8>, String> {
        let path = format!("{}/{}/snapshots/snapshot.bin", self.base_dir, self.key);
        let data = fs::read(&path).map_err(|e| e.to_string())?;
        logger::log_info(&format!(
            "[Storage] load_state: loaded {} bytes from '{}'",
            data.len(),
            path
        ));
        Ok(data)
    }

    /// Delete `state/<key>/`
    fn delete(&self) -> bool {
        let dir = format!("{}/{}", self.base_dir, self.key);
        let ok = fs::remove_dir_all(&dir).is_ok();
        logger::log_info(&format!("[Storage] delete: removed '{}', ok={}", dir, ok));
        ok
    }
}
