//! VCR recorder — captures LLM request/response pairs for replay in tests and demos.
//!
//! Only the *summaries* of requests and responses are stored (no raw bodies),
//! keeping cassette files lean and free of accidental PII.

use serde::{Deserialize, Serialize};

/// A single recorded interaction between the client and an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcrEntry {
    /// Short description of the request (e.g. model name + first 80 chars of prompt).
    pub request_summary: String,
    /// Short description of the response (e.g. first 80 chars of assistant reply).
    pub response_summary: String,
    /// ISO-8601 timestamp when the interaction was captured.
    pub timestamp: String,
    /// Round-trip latency in milliseconds.
    pub duration_ms: u64,
}

/// Records VCR cassette entries while `recording` is active.
///
/// Entries are held in memory until [`VcrRecorder::stop`] is called,
/// at which point they are returned for persistence via [`super::vcr_storage`].
#[derive(Debug, Default)]
pub struct VcrRecorder {
    /// Accumulated entries since the last [`start`](VcrRecorder::start) call.
    entries: Vec<VcrEntry>,
    /// Whether the recorder is currently capturing interactions.
    recording: bool,
}

impl VcrRecorder {
    /// Create a new, idle recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin recording. Any previously accumulated entries are cleared.
    pub fn start(&mut self) {
        tracing::debug!("VCR recording started");
        self.entries.clear();
        self.recording = true;
    }

    /// Stop recording and return all captured entries, consuming them.
    ///
    /// The recorder is left in a stopped state with an empty entry list.
    pub fn stop(&mut self) -> Vec<VcrEntry> {
        self.recording = false;
        tracing::debug!(
            "VCR recording stopped — {} entries captured",
            self.entries.len()
        );
        std::mem::take(&mut self.entries)
    }

    /// Append `entry` to the cassette if currently recording.
    ///
    /// Silently dropped when not recording.
    pub fn record_entry(&mut self, entry: VcrEntry) {
        if self.recording {
            tracing::debug!(
                duration_ms = entry.duration_ms,
                "VCR entry recorded: {}",
                entry.request_summary
            );
            self.entries.push(entry);
        }
    }

    /// Whether the recorder is currently active.
    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// Number of entries accumulated since the last [`start`](VcrRecorder::start).
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(req: &str, resp: &str) -> VcrEntry {
        VcrEntry {
            request_summary: req.to_string(),
            response_summary: resp.to_string(),
            timestamp: "2026-04-05T00:00:00Z".to_string(),
            duration_ms: 123,
        }
    }

    #[test]
    fn record_entry_only_when_recording() {
        let mut r = VcrRecorder::new();
        r.record_entry(make_entry("req1", "resp1")); // dropped — not recording
        assert_eq!(r.entry_count(), 0);

        r.start();
        r.record_entry(make_entry("req2", "resp2"));
        assert_eq!(r.entry_count(), 1);
    }

    #[test]
    fn stop_returns_and_clears_entries() {
        let mut r = VcrRecorder::new();
        r.start();
        r.record_entry(make_entry("a", "b"));
        r.record_entry(make_entry("c", "d"));

        let entries = r.stop();
        assert_eq!(entries.len(), 2);
        assert_eq!(r.entry_count(), 0);
        assert!(!r.is_recording());
    }

    #[test]
    fn start_clears_previous_entries() {
        let mut r = VcrRecorder::new();
        r.start();
        r.record_entry(make_entry("old", "data"));
        // Stop without consuming entries, then start again.
        r.recording = false; // simulate leftover state
        r.start();
        assert_eq!(r.entry_count(), 0);
    }
}
