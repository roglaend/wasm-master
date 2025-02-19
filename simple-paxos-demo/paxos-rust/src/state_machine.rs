use std::collections::HashMap;

/// Commands that can be applied to the state machine.
#[derive(Debug)]
pub enum Command {
    Set(String, String),
    Get(String),
}

/// Results returned by the state machine.
#[derive(Debug)]
pub enum CommandResult {
    /// For SET commands.
    Ack,
    /// For GET commands.
    Value(Option<String>),
    /// In case of error.
    Error(String),
}

/// A simple key–value store.
pub struct KVStore {
    pub store: HashMap<String, String>,
}

impl KVStore {
    pub fn new() -> Self {
        KVStore {
            store: HashMap::new(),
        }
    }

    /// Applies a command to the state machine.
    pub fn apply(&mut self, command: Command) -> CommandResult {
        match command {
            Command::Set(key, value) => {
                self.store.insert(key, value);
                CommandResult::Ack
            }
            Command::Get(key) => {
                let value = self.store.get(&key).cloned();
                CommandResult::Value(value)
            }
        }
    }
}

impl Command {
    /// Converts a string into a Command.
    /// Expected formats:
    /// - "SET key value"
    /// - "GET key"
    pub fn from_string(input: &str) -> Result<Self, String> {
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.is_empty() {
            return Err("Empty command".to_string());
        }
        match parts[0].to_uppercase().as_str() {
            "SET" => {
                if parts.len() != 3 {
                    Err("SET command must have 2 arguments".to_string())
                } else {
                    Ok(Command::Set(parts[1].to_string(), parts[2].to_string()))
                }
            }
            "GET" => {
                if parts.len() != 2 {
                    Err("GET command must have 1 argument".to_string())
                } else {
                    Ok(Command::Get(parts[1].to_string()))
                }
            }
            _ => Err("Unknown command".to_string()),
        }
    }
}
