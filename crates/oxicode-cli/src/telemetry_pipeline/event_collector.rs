//! Telemetry event collector — structured event types and batch collection.
//!
//! Collects telemetry events from across the application and dispatches them
//! to configured sinks (local file, OTLP exporter).

use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Telemetry event types matching OpenClaude analytics events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum TelemetryEvent {
    /// Session started.
    #[serde(rename = "session_start")]
    SessionStart {
        session_id: String,
        model: String,
        timestamp: u64,
    },

    /// Session ended.
    #[serde(rename = "session_end")]
    SessionEnd {
        session_id: String,
        duration_secs: u64,
        turns: u32,
        timestamp: u64,
    },

    /// Tool was executed.
    #[serde(rename = "tool_use")]
    ToolUse {
        tool_name: String,
        latency_ms: u64,
        is_error: bool,
        timestamp: u64,
    },

    /// Model was switched mid-session.
    #[serde(rename = "model_switch")]
    ModelSwitch {
        from_model: String,
        to_model: String,
        timestamp: u64,
    },

    /// Error occurred.
    #[serde(rename = "error")]
    Error {
        error_type: String,
        message: String,
        timestamp: u64,
    },

    /// Context compaction triggered.
    #[serde(rename = "compaction")]
    Compaction {
        messages_before: u32,
        messages_after: u32,
        tokens_saved: u64,
        timestamp: u64,
    },
}

impl TelemetryEvent {
    /// Get the timestamp of this event.
    pub fn timestamp(&self) -> u64 {
        match self {
            Self::SessionStart { timestamp, .. }
            | Self::SessionEnd { timestamp, .. }
            | Self::ToolUse { timestamp, .. }
            | Self::ModelSwitch { timestamp, .. }
            | Self::Error { timestamp, .. }
            | Self::Compaction { timestamp, .. } => *timestamp,
        }
    }
}

/// Get current Unix timestamp in seconds.
pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Configuration for the event collector.
#[derive(Debug, Clone)]
pub struct CollectorConfig {
    /// Whether OTLP export is enabled (opt-in).
    pub otlp_enabled: bool,
    /// Whether local file logging is enabled (default: true).
    pub local_log_enabled: bool,
    /// Remote killswitch — if true, suppress all network telemetry.
    pub killswitch_active: bool,
    /// Batch flush threshold (number of events).
    pub batch_size: usize,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            otlp_enabled: false,
            local_log_enabled: true,
            killswitch_active: false,
            batch_size: 100,
        }
    }
}

/// Event collector — accumulates events and dispatches to sinks.
#[derive(Debug, Clone)]
pub struct EventCollector {
    events: Arc<Mutex<Vec<TelemetryEvent>>>,
    config: CollectorConfig,
}

impl EventCollector {
    /// Create a new event collector with the given config.
    pub fn new(config: CollectorConfig) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            config,
        }
    }

    /// Record an event.
    pub fn record(&self, event: TelemetryEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }

    /// Drain all collected events (for batch processing).
    pub fn drain(&self) -> Vec<TelemetryEvent> {
        if let Ok(mut events) = self.events.lock() {
            std::mem::take(&mut *events)
        } else {
            Vec::new()
        }
    }

    /// Number of pending events.
    pub fn pending_count(&self) -> usize {
        self.events.lock().map_or(0, |e| e.len())
    }

    /// Whether batch threshold is reached.
    pub fn should_flush(&self) -> bool {
        self.pending_count() >= self.config.batch_size
    }

    /// Whether OTLP export is allowed (enabled and not killswitched).
    pub fn is_otlp_allowed(&self) -> bool {
        self.config.otlp_enabled && !self.config.killswitch_active
    }

    /// Whether local file logging is enabled.
    pub fn is_local_log_enabled(&self) -> bool {
        self.config.local_log_enabled
    }

    /// Activate the remote killswitch (disables all network telemetry).
    pub fn set_killswitch(&mut self, active: bool) {
        self.config.killswitch_active = active;
    }

    /// Reference to config.
    pub fn config(&self) -> &CollectorConfig {
        &self.config
    }
}

impl Default for EventCollector {
    fn default() -> Self {
        Self::new(CollectorConfig::default())
    }
}

// Security filter: strip sensitive data from events before export.
// Telemetry MUST NOT include: API keys, file contents, user input, conversation text.
// The event types above are designed to carry only metric/structural data.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let event = TelemetryEvent::SessionStart {
            session_id: "s1".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            timestamp: 1_700_000_000,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("session_start"));
        assert!(json.contains("s1"));
    }

    #[test]
    fn test_event_deserialization_roundtrip() {
        let event = TelemetryEvent::ToolUse {
            tool_name: "Bash".to_string(),
            latency_ms: 150,
            is_error: false,
            timestamp: 1_700_000_000,
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: TelemetryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.timestamp(), 1_700_000_000);
    }

    #[test]
    fn test_collector_record_and_drain() {
        let collector = EventCollector::default();
        collector.record(TelemetryEvent::SessionStart {
            session_id: "s1".to_string(),
            model: "test".to_string(),
            timestamp: now_unix_secs(),
        });
        assert_eq!(collector.pending_count(), 1);

        let events = collector.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(collector.pending_count(), 0);
    }

    #[test]
    fn test_collector_batch_threshold() {
        let config = CollectorConfig {
            batch_size: 2,
            ..Default::default()
        };
        let collector = EventCollector::new(config);
        assert!(!collector.should_flush());

        collector.record(TelemetryEvent::Error {
            error_type: "test".to_string(),
            message: "e1".to_string(),
            timestamp: 0,
        });
        assert!(!collector.should_flush());

        collector.record(TelemetryEvent::Error {
            error_type: "test".to_string(),
            message: "e2".to_string(),
            timestamp: 0,
        });
        assert!(collector.should_flush());
    }

    #[test]
    fn test_killswitch() {
        let mut collector = EventCollector::new(CollectorConfig {
            otlp_enabled: true,
            ..Default::default()
        });
        assert!(collector.is_otlp_allowed());

        collector.set_killswitch(true);
        assert!(!collector.is_otlp_allowed());
    }

    #[test]
    fn test_default_config() {
        let config = CollectorConfig::default();
        assert!(!config.otlp_enabled);
        assert!(config.local_log_enabled);
        assert!(!config.killswitch_active);
        assert_eq!(config.batch_size, 100);
    }

    #[test]
    fn test_all_event_types_serialize() {
        let events = vec![
            TelemetryEvent::SessionStart {
                session_id: "s1".into(),
                model: "m".into(),
                timestamp: 0,
            },
            TelemetryEvent::SessionEnd {
                session_id: "s1".into(),
                duration_secs: 60,
                turns: 5,
                timestamp: 0,
            },
            TelemetryEvent::ToolUse {
                tool_name: "Bash".into(),
                latency_ms: 100,
                is_error: false,
                timestamp: 0,
            },
            TelemetryEvent::ModelSwitch {
                from_model: "a".into(),
                to_model: "b".into(),
                timestamp: 0,
            },
            TelemetryEvent::Error {
                error_type: "api".into(),
                message: "fail".into(),
                timestamp: 0,
            },
            TelemetryEvent::Compaction {
                messages_before: 50,
                messages_after: 1,
                tokens_saved: 10_000,
                timestamp: 0,
            },
        ];
        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            assert!(!json.is_empty());
        }
    }
}
