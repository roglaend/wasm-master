use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tracing::Level as TracingLevel;
use tracing::{debug, error, info, warn};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::{EnvFilter, fmt};

use crate::bindings::paxos::default::logger::Level;

pub(crate) fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = fmt().with_env_filter(filter).finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global tracing subscriber");
}

/// A host-side logger that writes logs to a node-specific file,
/// a shared global file, and also keeps an in-memory log with incremental offsets.
pub struct HostNode {
    pub node_id: u64,
    pub address: String,
    pub role: u64,
}

pub struct HostLogger {
    pub node: HostNode,
    pub node_file: Mutex<std::fs::File>,
    pub global_file: Arc<Mutex<std::fs::File>>,
    pub log_entries: Mutex<Vec<(u64, String)>>, // (offset, message)
    pub next_offset: AtomicU64,
    pub log_level: LevelFilter,
}

impl HostLogger {
    /// Create a new HostLogger for a given node.
    /// It creates node-specific and global log files in a workspace "logs" directory.
    pub fn new_from_workspace(node: HostNode) -> Self {
        // Compute the workspace directory.
        let binding = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_dir = binding.parent().expect("Failed to get workspace directory");
        let logs_dir = workspace_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).expect("Failed to create logs folder");

        // Build file paths.
        let node_file_path = logs_dir.join(format!("node{}.log", node.node_id));
        let global_file_path = logs_dir.join("global.log");

        let node_file_path_str = node_file_path.to_str().expect("Invalid node file path");

        let global_file = Arc::new(Mutex::new(
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(global_file_path)
                .expect("Failed to open global log file"),
        ));

        Self::new(node, node_file_path_str, global_file)
    }

    /// Create a new HostLogger given a node, node file path, and a shared global file.
    pub fn new(
        node: HostNode,
        node_file_path: &str,
        global_file: Arc<Mutex<std::fs::File>>,
    ) -> Self {
        // Parse the RUST_LOG variable into a LevelFilter; default to Info if not set.
        let log_level = std::env::var("RUST_LOG")
            .ok()
            .and_then(|lvl| lvl.parse::<LevelFilter>().ok())
            .unwrap_or(LevelFilter::INFO);

        let node_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(node_file_path)
            .expect("Failed to open node-specific log file");

        HostLogger {
            node,
            node_file: Mutex::new(node_file),
            global_file,
            log_entries: Mutex::new(Vec::new()),
            next_offset: AtomicU64::new(0),
            log_level,
        }
    }

    /// Append a log entry to the in-memory log and return its offset.
    fn append_log(&self, msg: String) -> u64 {
        let offset = self.next_offset.fetch_add(1, Ordering::SeqCst);
        self.log_entries.lock().unwrap().push((offset, msg));
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

    /// Convert a tracing Level into a numeric value.
    /// Lower numbers represent more severe levels.
    fn level_to_usize(level: TracingLevel) -> usize {
        match level {
            TracingLevel::ERROR => 1,
            TracingLevel::WARN => 2,
            TracingLevel::INFO => 3,
            TracingLevel::DEBUG => 4,
            TracingLevel::TRACE => 5,
        }
    }

    /// Convert a LevelFilter into a numeric value.
    /// Lower numbers represent stricter filtering.
    fn level_filter_to_usize(filter: LevelFilter) -> usize {
        match filter {
            LevelFilter::OFF => 6, // essentially never log
            LevelFilter::ERROR => 1,
            LevelFilter::WARN => 2,
            LevelFilter::INFO => 3,
            LevelFilter::DEBUG => 4,
            LevelFilter::TRACE => 5,
        }
    }

    /// Log a message at the specified level.
    fn log(&self, level: Level, msg: String) {
        // Map our custom Level into a tracing::Level.
        let tracing_level = match level {
            Level::Debug => TracingLevel::DEBUG,
            Level::Info => TracingLevel::INFO,
            Level::Warn => TracingLevel::WARN,
            Level::Error => TracingLevel::ERROR,
        };

        // Only log the message if its severity meets or exceeds the threshold.
        if Self::level_to_usize(tracing_level) > Self::level_filter_to_usize(self.log_level) {
            return;
        }

        // Build a formatted log string using details from the HostNode.
        let formatted = format!(
            "[node {} | {} | {:?}] {}",
            self.node.node_id, self.node.address, self.node.role, msg
        );

        // Emit the log via the tracing macros.
        match level {
            Level::Debug => debug!("{}", formatted),
            Level::Info => info!("{}", formatted),
            Level::Warn => warn!("{}", formatted),
            Level::Error => error!("{}", formatted),
        }

        // Build the log line to be written to disk.
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

        // Append to the in-memory log.
        let _ = self.append_log(msg);
    }
}
