//! Local file logger — append telemetry events as NDJSON to a local file.
//!
//! Always on (no network, no opt-in needed). Writes to `~/.oxicode/telemetry.ndjson`.
//! Rotates at 10MB, keeping the last 3 files.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use super::event_collector::TelemetryEvent;

/// Maximum log file size before rotation (10 MB).
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Number of rotated files to keep.
const MAX_ROTATED_FILES: usize = 3;

/// Log file name.
const LOG_FILENAME: &str = "telemetry.ndjson";

/// Local NDJSON file logger for telemetry events.
pub struct LocalFileLogger {
    /// Directory containing the log file.
    log_dir: PathBuf,
}

impl LocalFileLogger {
    /// Create a new logger writing to `~/.oxicode/`.
    pub fn new() -> Self {
        let log_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".oxicode");
        Self { log_dir }
    }

    /// Create a logger with a custom directory (for testing).
    pub fn with_dir(dir: PathBuf) -> Self {
        Self { log_dir: dir }
    }

    /// Path to the current log file.
    fn log_path(&self) -> PathBuf {
        self.log_dir.join(LOG_FILENAME)
    }

    /// Path to a rotated log file (e.g. telemetry.ndjson.1).
    fn rotated_path(&self, index: usize) -> PathBuf {
        self.log_dir.join(format!("{LOG_FILENAME}.{index}"))
    }

    /// Write a batch of events to the log file (NDJSON format).
    pub fn write_events(&self, events: &[TelemetryEvent]) -> Result<usize, String> {
        if events.is_empty() {
            return Ok(0);
        }

        // Ensure log directory exists.
        fs::create_dir_all(&self.log_dir)
            .map_err(|e| format!("Failed to create log dir: {e}"))?;

        // Check if rotation is needed before writing.
        self.maybe_rotate()?;

        let path = self.log_path();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("Failed to open log file: {e}"))?;

        let mut written = 0;
        for event in events {
            if let Ok(json) = serde_json::to_string(event) {
                writeln!(file, "{json}")
                    .map_err(|e| format!("Failed to write event: {e}"))?;
                written += 1;
            }
        }

        Ok(written)
    }

    /// Write a single event.
    pub fn write_event(&self, event: &TelemetryEvent) -> Result<(), String> {
        self.write_events(std::slice::from_ref(event))?;
        Ok(())
    }

    /// Rotate log file if it exceeds the size limit.
    fn maybe_rotate(&self) -> Result<(), String> {
        let path = self.log_path();
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

        if size < MAX_FILE_SIZE {
            return Ok(());
        }

        tracing::info!(
            size_mb = size / (1024 * 1024),
            "Rotating telemetry log file"
        );

        // Shift existing rotated files: .3 -> delete, .2 -> .3, .1 -> .2, current -> .1
        for i in (1..MAX_ROTATED_FILES).rev() {
            let from = self.rotated_path(i);
            let to = self.rotated_path(i + 1);
            if from.exists() {
                let _ = fs::rename(&from, &to);
            }
        }

        // Delete the oldest if it exists.
        let oldest = self.rotated_path(MAX_ROTATED_FILES);
        if oldest.exists() {
            let _ = fs::remove_file(&oldest);
        }

        // Rotate current -> .1
        let first_rotated = self.rotated_path(1);
        fs::rename(&path, &first_rotated)
            .map_err(|e| format!("Failed to rotate log file: {e}"))?;

        Ok(())
    }

    /// Current log file size in bytes.
    pub fn current_size(&self) -> u64 {
        fs::metadata(self.log_path()).map(|m| m.len()).unwrap_or(0)
    }
}

impl Default for LocalFileLogger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::event_collector::{TelemetryEvent, now_unix_secs};
    use tempfile::TempDir;

    fn make_logger(dir: &TempDir) -> LocalFileLogger {
        LocalFileLogger::with_dir(dir.path().to_path_buf())
    }

    fn sample_event() -> TelemetryEvent {
        TelemetryEvent::SessionStart {
            session_id: "test-session".to_string(),
            model: "test-model".to_string(),
            timestamp: now_unix_secs(),
        }
    }

    #[test]
    fn test_write_single_event() {
        let dir = TempDir::new().unwrap();
        let logger = make_logger(&dir);
        logger.write_event(&sample_event()).unwrap();

        let content = fs::read_to_string(dir.path().join(LOG_FILENAME)).unwrap();
        assert!(content.contains("session_start"));
        assert!(content.contains("test-session"));
    }

    #[test]
    fn test_write_batch() {
        let dir = TempDir::new().unwrap();
        let logger = make_logger(&dir);
        let events = vec![sample_event(), sample_event(), sample_event()];
        let written = logger.write_events(&events).unwrap();
        assert_eq!(written, 3);

        let content = fs::read_to_string(dir.path().join(LOG_FILENAME)).unwrap();
        let lines: Vec<_> = content.lines().collect();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_write_empty_batch() {
        let dir = TempDir::new().unwrap();
        let logger = make_logger(&dir);
        let written = logger.write_events(&[]).unwrap();
        assert_eq!(written, 0);
    }

    #[test]
    fn test_append_mode() {
        let dir = TempDir::new().unwrap();
        let logger = make_logger(&dir);
        logger.write_event(&sample_event()).unwrap();
        logger.write_event(&sample_event()).unwrap();

        let content = fs::read_to_string(dir.path().join(LOG_FILENAME)).unwrap();
        let lines: Vec<_> = content.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_current_size() {
        let dir = TempDir::new().unwrap();
        let logger = make_logger(&dir);
        assert_eq!(logger.current_size(), 0);

        logger.write_event(&sample_event()).unwrap();
        assert!(logger.current_size() > 0);
    }

    #[test]
    fn test_ndjson_format() {
        let dir = TempDir::new().unwrap();
        let logger = make_logger(&dir);
        logger.write_event(&sample_event()).unwrap();

        let content = fs::read_to_string(dir.path().join(LOG_FILENAME)).unwrap();
        for line in content.lines() {
            // Each line should be valid JSON.
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }
    }

    #[test]
    fn test_log_constants() {
        assert_eq!(MAX_FILE_SIZE, 10 * 1024 * 1024);
        assert_eq!(MAX_ROTATED_FILES, 3);
        assert_eq!(LOG_FILENAME, "telemetry.ndjson");
    }
}
