use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::SystemTime;

const MAX_ENTRIES: usize = 100;

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub timestamp: u64,
    pub level: String,
    pub message: String,
    pub tunnel_name: Option<String>,
}

pub struct ErrorLog {
    entries: Mutex<VecDeque<LogEntry>>,
}

impl Default for ErrorLog {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorLog {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
        }
    }

    pub fn push(&self, level: &str, message: String, tunnel_name: Option<String>) {
        let entry = LogEntry {
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            level: level.to_string(),
            message,
            tunnel_name,
        };
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= MAX_ENTRIES {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    pub fn error(&self, message: String, tunnel_name: Option<String>) {
        self.push("error", message, tunnel_name);
    }

    #[allow(dead_code)]
    pub fn warn(&self, message: String, tunnel_name: Option<String>) {
        self.push("warn", message, tunnel_name);
    }

    pub fn info(&self, message: String, tunnel_name: Option<String>) {
        self.push("info", message, tunnel_name);
    }

    pub fn get_all(&self) -> Vec<LogEntry> {
        self.entries.lock().unwrap().iter().cloned().collect()
    }

    pub fn error_count(&self) -> usize {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.level == "error")
            .count()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }
}
