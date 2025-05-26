use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use byteorder::{BigEndian, ByteOrder};

#[derive(Debug)]
pub enum StorageRequest {
    SaveState {
        key: String,
        data: Vec<u8>,
    },
    SaveChange {
        key: String,
        data: Vec<u8>,
    },
    Checkpoint {
        key: String,
        data: Vec<u8>,
        timestamp: String,
        max_snapshots: u64,
    },
}

pub struct HostStorage {
    pub base_dir: PathBuf,
    sender: mpsc::UnboundedSender<StorageRequest>,
}

impl HostStorage {
    pub fn new(base_dir: PathBuf) -> Arc<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel::<StorageRequest>();

        std::fs::create_dir_all(&base_dir).expect("Failed to create base storage directory");

        let storage = Arc::new(Self {
            base_dir: base_dir.clone(),
            sender: tx,
        });

        let background = storage.clone();
        tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                background.handle_request(req).await;
            }
        });

        storage
    }

    pub fn enqueue(&self, req: StorageRequest) {
        let _ = self.sender.send(req);
    }

    pub async fn handle_request(&self, request: StorageRequest) {
        match request {
            StorageRequest::SaveState { key, data } => {
                let path = self.path_for(&key, "current-state.bin");
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent).await;
                }
                let _ = fs::write(path, data).await;
            }

            StorageRequest::SaveChange { key, data } => {
                let path = self.path_for(&key, "changelog.bin");
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent).await;
                }
                if let Ok(mut file) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .await
                {
                    let _ = file.write_u32(data.len() as u32).await;
                    let _ = file.write_all(&data).await;
                }
            }

            StorageRequest::Checkpoint {
                key,
                data,
                timestamp,
                max_snapshots,
            } => {
                let snapshot_dir = self.base_dir.join(&key).join("snapshots");
                let tmp = snapshot_dir.join(format!("snapshot-{}.tmp", timestamp));
                let final_path = snapshot_dir.join(format!("snapshot-{}.bin", timestamp));

                if let Some(parent) = tmp.parent() {
                    let _ = fs::create_dir_all(parent).await;
                }

                if let Ok(mut file) = fs::File::create(&tmp).await {
                    let _ = file.write_all(&data).await;
                    let _ = file.sync_all().await;
                    let _ = fs::rename(&tmp, &final_path).await;
                }

                let changelog_path = self.path_for(&key, "changelog.bin");
                let _ = fs::File::create(&changelog_path).await;

                let _ = self.prune_snapshots(&key, max_snapshots).await;
            }
        }
    }

    pub async fn load_current_state(&self, key: &str) -> Result<Vec<u8>, String> {
        let path = self.path_for(key, "current-state.bin");
        fs::read(path).await.map_err(|e| e.to_string())
    }

    pub async fn list_snapshots(&self, key: &str) -> Vec<String> {
        let dir = self.base_dir.join(key).join("snapshots");

        let mut files = Vec::new();

        if let Ok(mut entries) = fs::read_dir(&dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let fname = entry.file_name().to_string_lossy().to_string();
                if fname.starts_with("snapshot-") && fname.ends_with(".bin") {
                    files.push(fname);
                }
            }
        }

        files.sort();
        files
    }

    pub async fn prune_snapshots(&self, key: &str, retain: u64) -> Result<(), String> {
        let mut files = self.list_snapshots(key).await;
        let dir = self.base_dir.join(key).join("snapshots");

        while files.len() as u64 > retain {
            if let Some(oldest) = files.first() {
                let path = dir.join(oldest);
                let _ = fs::remove_file(path).await;
                files.remove(0);
            }
        }

        Ok(())
    }

    pub async fn load_state_and_changes(
        &self,
        key: &str,
        num_snapshots: u64,
    ) -> Result<(Vec<Vec<u8>>, Vec<Vec<u8>>), String> {
        let all = self.list_snapshots(key).await;
        let dir = self.base_dir.join(key).join("snapshots");

        let start = all.len().saturating_sub(num_snapshots as usize);
        let selected = &all[start..];

        let mut snapshots = vec![];
        for name in selected {
            let path = dir.join(name);
            let data = fs::read(&path).await.map_err(|e| e.to_string())?;
            snapshots.push(data);
        }

        let changelog_path = self.path_for(key, "changelog.bin");
        let mut changes = vec![];

        if let Ok(file) = fs::File::open(&changelog_path).await {
            let mut reader = BufReader::new(file);

            loop {
                let mut len_buf = [0u8; 4];
                if reader.read_exact(&mut len_buf).await.is_err() {
                    break;
                }
                let len = BigEndian::read_u32(&len_buf) as usize;
                let mut buf = vec![0u8; len];
                reader
                    .read_exact(&mut buf)
                    .await
                    .map_err(|e| e.to_string())?;
                changes.push(buf);
            }
        }

        Ok((snapshots, changes))
    }

    pub fn delete(&self, key: &str) -> bool {
        let dir = self.base_dir.join(key);
        std::fs::remove_dir_all(dir).is_ok()
    }

    fn path_for(&self, key: &str, file: &str) -> PathBuf {
        self.base_dir.join(key).join(file)
    }
}
