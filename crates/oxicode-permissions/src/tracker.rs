use std::sync::Mutex;

use chrono::{DateTime, Utc};

/// Tracks permission denials for analytics and user review.
pub struct DenialTracker {
    entries: Mutex<Vec<DenialEntry>>,
}

struct DenialEntry {
    tool_name: String,
    reason: String,
    timestamp: DateTime<Utc>,
}

impl DenialTracker {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Record a denial event.
    pub fn record(&self, tool_name: &str, reason: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(DenialEntry {
                tool_name: tool_name.to_string(),
                reason: reason.to_string(),
                timestamp: Utc::now(),
            });
        }
    }

    /// Get denial history as (tool_name, reason, timestamp).
    pub fn history(&self) -> Vec<(String, String, DateTime<Utc>)> {
        self.entries
            .lock()
            .map(|entries| {
                entries
                    .iter()
                    .map(|e| (e.tool_name.clone(), e.reason.clone(), e.timestamp))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Number of recorded denials.
    pub fn count(&self) -> usize {
        self.entries.lock().map_or(0, |e| e.len())
    }
}

impl Default for DenialTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_history() {
        let tracker = DenialTracker::new();
        tracker.record("bash", "dangerous command");
        tracker.record("file_write", "protected path");

        assert_eq!(tracker.count(), 2);

        let history = tracker.history();
        assert_eq!(history[0].0, "bash");
        assert_eq!(history[1].0, "file_write");
    }

    #[test]
    fn test_empty_history() {
        let tracker = DenialTracker::new();
        assert_eq!(tracker.count(), 0);
        assert!(tracker.history().is_empty());
    }
}
