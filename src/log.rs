use std::collections::VecDeque;
use std::time::SystemTime;

const CAPACITY: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: SystemTime,
    pub kind: LogKind,
    pub text: String,
}

#[derive(Default)]
pub struct LogBuffer {
    entries: VecDeque<LogEntry>,
}

impl LogBuffer {
    pub fn push(&mut self, kind: LogKind, text: impl Into<String>) {
        if self.entries.len() >= CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(LogEntry {
            timestamp: SystemTime::now(),
            kind,
            text: text.into(),
        });
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

pub fn classify(text: &str) -> LogKind {
    let lower = text.to_lowercase();
    if lower.contains("error") || lower.contains("failed") || lower.contains("panic") {
        LogKind::Error
    } else if lower.contains("warn") || lower.contains("unsaved") || lower.contains("invalid") {
        LogKind::Warn
    } else {
        LogKind::Info
    }
}

pub fn relative_age(t: SystemTime) -> String {
    let elapsed = SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if elapsed < 1 {
        "just now".to_string()
    } else if elapsed < 60 {
        format!("{}s ago", elapsed)
    } else if elapsed < 3600 {
        format!("{}m ago", elapsed / 60)
    } else if elapsed < 86_400 {
        format!("{}h ago", elapsed / 3600)
    } else {
        format!("{}d ago", elapsed / 86_400)
    }
}
