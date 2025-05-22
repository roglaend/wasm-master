use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, error, info, warn};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::{EnvFilter, fmt};

use crate::bindings::paxos::default::logger::Level;
use crate::bindings::paxos::default::paxos_types::PaxosRole;

pub(crate) fn init_tracing_with(level: Level) {
    let level_str = match level {
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
    };
    let filter = EnvFilter::new(level_str);
    let subscriber = fmt().with_env_filter(filter).finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global tracing subscriber");
}

/// A host-side logger that writes logs to a node-specific file,
/// a shared global file, and also keeps an in-memory log with incremental offsets.
pub struct HostNode {
    pub node_id: u64,
    pub address: String,
    pub role: PaxosRole,
}

pub struct HostLogger {
    pub node: HostNode,
    pub node_file: Mutex<std::fs::File>,
    pub _log_entries: Mutex<Vec<(u64, String)>>, // (offset, message)
    pub _next_offset: AtomicU64,
    pub log_level: LevelFilter,
}

impl HostLogger {
    /// Create a new HostLogger for a given node.
    /// It creates node-specific and global log files in a workspace "logs" directory.
    pub fn new_from_workspace(node: HostNode, level: Level) -> Self {
        // Compute the workspace directory.
        let binding = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_dir = binding.parent().expect("Failed to get workspace directory");
        let logs_dir = workspace_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).expect("Failed to create logs folder");

        // Build file paths.
        let node_file_path = logs_dir.join(format!("node{}.log", node.node_id));
        let node_file_path_str = node_file_path.to_str().expect("Invalid node file path");

        Self::new(node, node_file_path_str, level)
    }

    /// Create a new HostLogger given a node, node file path, and a shared global file.
    pub fn new(node: HostNode, node_file_path: &str, level: Level) -> Self {
        // Map our custom "Level" to a "LevelFilter"
        let log_level = match level {
            Level::Debug => LevelFilter::DEBUG,
            Level::Info => LevelFilter::INFO,
            Level::Warn => LevelFilter::WARN,
            Level::Error => LevelFilter::ERROR,
        };

        let node_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(node_file_path)
            .expect("Failed to open node-specific log file");

        HostLogger {
            node,
            node_file: Mutex::new(node_file),
            _log_entries: Mutex::new(Vec::new()),
            _next_offset: AtomicU64::new(0),
            log_level,
        }
    }

    /// Append a log entry to the in-memory log and return its offset.
    fn _append_log(&self, msg: String) -> u64 {
        let offset = self._next_offset.fetch_add(1, Ordering::SeqCst);
        self._log_entries.lock().unwrap().push((offset, msg));
        offset
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

    /// Log a message at the specified level.
    fn log(&self, level: Level, msg: String) {
        // map to a LevelFilter for comparison
        let event_filter = match level {
            Level::Error => LevelFilter::ERROR,
            Level::Warn => LevelFilter::WARN,
            Level::Info => LevelFilter::INFO,
            Level::Debug => LevelFilter::DEBUG,
        };
        if event_filter > self.log_level {
            return;
        }

        let console_msg = format!(
            "[node {} | {} | {:?}] {}",
            self.node.node_id, self.node.address, self.node.role, msg
        );
        let file_msg = format!(
            "[node {} | {} | {:?}][{:?}] {}\n",
            self.node.node_id, self.node.address, self.node.role, level, msg
        );

        match level {
            Level::Debug => debug!("{}", console_msg),
            Level::Info => info!("{}", console_msg),
            Level::Warn => warn!("{}", console_msg),
            Level::Error => error!("{}", console_msg),
        }

        if let Ok(mut nf) = self.node_file.lock() {
            let _ = nf.write_all(file_msg.as_bytes());
        }

        // let _ = self.append_log(msg); // TODO: Useful?
    }
}
