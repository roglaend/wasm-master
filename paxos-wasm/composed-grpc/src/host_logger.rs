use crate::paxos_bindings::paxos::default::logger::Level;
use crate::paxos_bindings::paxos::default::paxos_types::Node;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, warn};

/// A host-side logger that writes logs to a node-specific file,
/// a shared global file, and also keeps an in-memory log with incremental offsets.
pub struct HostLogger {
    pub node: Node,
    pub node_file: Mutex<std::fs::File>,
    pub global_file: Arc<Mutex<std::fs::File>>,
    pub log_entries: Mutex<Vec<(u64, String)>>, // (offset, message)
    pub next_offset: AtomicU64,
}

impl HostLogger {
    /// Create a new HostLogger for a given node.
    ///
    /// * `node_id` - The identifier for this node.
    /// * `node_file_path` - The file path for this node’s log (e.g. "./logs/node1.log").
    /// * `global_file` - An Arc‑wrapped Mutex to a global log file.
    pub fn new_from_workspace(node: Node) -> Self {
        // Compute the workspace directory (the parent of Cargo.toml's directory).
        let binding = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_dir = binding.parent().expect("Failed to get workspace directory");
        // Define the logs directory relative to the workspace.
        let logs_dir = workspace_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).expect("Failed to create logs folder");

        // Build file paths.
        let node_file_path = logs_dir.join(format!("node{}.log", node.node_id));
        let global_file_path = logs_dir.join("global.log");

        let node_file_path_str = node_file_path.to_str().expect("Invalid node file path");

        let global_file = Arc::new(Mutex::new(
            OpenOptions::new()
                .append(true)
                .create(true)
                .open(global_file_path)
                .expect("Failed to open global log file"),
        ));

        Self::new(node, node_file_path_str, global_file)
    }

    // TODO: Maybe change this later to be per paxos phase logs, and a global log for the node.
    // TODO: Then make the "truly global" log be created by a third

    /// Create a new HostLogger given a node ID, node file path, and a shared global file.
    pub fn new(node: Node, node_file_path: &str, global_file: Arc<Mutex<std::fs::File>>) -> Self {
        let node_file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(node_file_path)
            .expect("Failed to open node-specific log file");
        HostLogger {
            node,
            node_file: Mutex::new(node_file),
            global_file,
            log_entries: Mutex::new(Vec::new()),
            next_offset: AtomicU64::new(0),
        }
    }
    /// Append a log entry to the in-memory log and return its offset.
    fn append_log(&self, msg: String) -> u64 {
        let offset = self.next_offset.fetch_add(1, Ordering::SeqCst);
        self.log_entries.lock().unwrap().push((offset, msg));
        offset
    }

    /// Returns all log entries with offsets greater than `last_offset`
    /// and the new highest offset.
    pub fn get_logs(&self, last_offset: u64) -> (Vec<(u64, String)>, u64) {
        let logs = self.log_entries.lock().unwrap();
        let new_entries: Vec<(u64, String)> = logs
            .iter()
            .filter(|(offset, _)| *offset >= last_offset)
            .cloned()
            .collect();
        let new_offset = new_entries
            .last()
            .map(|(offset, _)| *offset)
            .unwrap_or(last_offset);
        (new_entries, new_offset)
    }

    /// Logs an info-level message.
    pub fn log_info(&self, msg: String) {
        self.log(Level::Info, msg);
    }

    /// Logs a warn-level message.
    pub fn log_warn(&self, msg: String) {
        self.log(Level::Warn, msg);
    }

    /// Logs an error-level message.
    pub fn log_error(&self, msg: String) {
        self.log(Level::Error, msg);
    }

    /// Logs a debug-level message.
    pub fn log_debug(&self, msg: String) {
        self.log(Level::Debug, msg);
    }

    // TODO: Make the logging async!

    /// Log a message at the specified level.
    fn log(&self, level: Level, msg: String) {
        // Build a formatted log string using details from the Node.
        let formatted = format!(
            "[node {} | {} | {:?}] {}",
            self.node.node_id, self.node.address, self.node.role, msg
        );
        // TODO: Might be a bit too verbose?
        match level {
            Level::Debug => debug!("{}", formatted),
            Level::Info => info!("{}", formatted),
            Level::Warn => warn!("{}", formatted),
            Level::Error => error!("{}", formatted),
        }
        // TODO: Might be a bit too verbose?
        // Build the log line to be written to file.
        let log_line = format!(
            "[node {} | {} | {:?}][{:?}] {}\n",
            self.node.node_id, self.node.address, self.node.role, level, msg
        );
        // Write to the node-specific file.
        if let Ok(mut file) = self.node_file.lock() {
            let _ = file.write_all(log_line.as_bytes());
        }
        // Write to the global file.
        if let Ok(mut file) = self.global_file.lock() {
            let _ = file.write_all(log_line.as_bytes());
        }
        // Append to in-memory log.
        let _ = self.append_log(msg);
    }
}
