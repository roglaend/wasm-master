#[cfg(test)]
mod tests {
    use crate::host_logger::HostLogger;
    use crate::paxos_bindings::paxos::default::paxos_types::{Node, PaxosRole};

    use std::fs;
    use std::sync::{Arc, Mutex};
    use tempfile::NamedTempFile;

    #[test]
    fn test_host_logger_in_memory() {
        // Create temporary files for node-specific and global logs.
        let node_temp = NamedTempFile::new().expect("failed to create node temp file");
        let global_temp = NamedTempFile::new().expect("failed to create global temp file");
        let global_file = Arc::new(Mutex::new(global_temp.reopen().unwrap()));

        let logger = HostLogger::new(
            Node {
                node_id: 1,
                address: "address".to_string(),
                role: PaxosRole::Coordinator,
            },
            node_temp.path().to_str().unwrap(),
            global_file.clone(),
        );

        logger.log_info("Test info message".to_string());
        logger.log_warn("Test warn message".to_string());

        // Check in-memory log.
        let (entries, new_offset) = logger.get_logs(0);
        assert_eq!(entries.len(), 2);
        assert_eq!(new_offset, entries.last().unwrap().0);
        assert!(entries[0].1.contains("Test info message"));
        assert!(entries[1].1.contains("Test warn message"));
    }

    #[test]
    fn test_host_logger_file_output() {
        // Use temporary files.
        let node_temp = NamedTempFile::new().expect("failed to create node temp file");
        let global_temp = NamedTempFile::new().expect("failed to create global temp file");
        let global_file = Arc::new(Mutex::new(global_temp.reopen().unwrap()));

        let logger = HostLogger::new(
            Node {
                node_id: 2,
                address: "address 2".to_string(),
                role: PaxosRole::Proposer,
            },
            node_temp.path().to_str().unwrap(),
            global_file.clone(),
        );

        logger.log_error("Test error message".to_string());

        // Read node-specific log.
        let node_contents = fs::read_to_string(node_temp.path()).unwrap();
        assert!(node_contents.contains("Test error message"));
        // Read global log.
        let global_contents = fs::read_to_string(global_temp.path()).unwrap();
        assert!(global_contents.contains("Test error message"));
    }
}
