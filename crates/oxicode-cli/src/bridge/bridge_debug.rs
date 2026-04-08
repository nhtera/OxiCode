//! Bridge debug logger — logs all bridge messages to `~/.oxicode/bridge-debug.log`.
//!
//! Features:
//! - Message logging with timestamps and direction (send/recv)
//! - Connection event logging (connect, disconnect, reconnect)
//! - Stats tracking (message counts, avg latency, error count)
//! - Log rotation at 10 MB (keeps last 3 files)

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Maximum log file size before rotation (10 MB).
const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024;

/// Number of rotated log files to keep.
const MAX_ROTATED_FILES: usize = 3;

/// Direction of a bridge message.
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Send,
    Recv,
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Send => write!(f, "SEND"),
            Self::Recv => write!(f, "RECV"),
        }
    }
}

/// Aggregate stats for debug inspection.
#[derive(Debug, Clone)]
pub struct BridgeDebugStats {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub errors: u64,
    pub connection_events: u64,
    /// Average message processing latency in microseconds (0 if no samples).
    pub avg_latency_us: u64,
}

/// Bridge debug logger — writes timestamped entries to a log file.
pub struct BridgeDebugLogger {
    log_path: PathBuf,
    enabled: bool,
    sent: AtomicU64,
    received: AtomicU64,
    errors: AtomicU64,
    connection_events: AtomicU64,
    latency_sum_us: AtomicU64,
    latency_count: AtomicU64,
    started_at: Instant,
}

impl BridgeDebugLogger {
    /// Create a new logger.
    ///
    /// If `enabled` is false, all log methods are no-ops (zero overhead).
    pub fn new(log_path: PathBuf, enabled: bool) -> Self {
        Self {
            log_path,
            enabled,
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            connection_events: AtomicU64::new(0),
            latency_sum_us: AtomicU64::new(0),
            latency_count: AtomicU64::new(0),
            started_at: Instant::now(),
        }
    }

    /// Create a disabled (no-op) logger.
    pub fn disabled() -> Self {
        Self::new(PathBuf::new(), false)
    }

    /// Log a bridge message with direction and content summary.
    pub fn log_message(&self, direction: Direction, message: &str) {
        if !self.enabled {
            return;
        }

        match direction {
            Direction::Send => self.sent.fetch_add(1, Ordering::Relaxed),
            Direction::Recv => self.received.fetch_add(1, Ordering::Relaxed),
        };

        // Truncate long messages for the log (snap to char boundary to avoid
        // panicking on multi-byte UTF-8).
        let summary = if message.len() > 500 {
            let mut end = 500;
            while end > 0 && !message.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}... ({} bytes)", &message[..end], message.len())
        } else {
            message.to_string()
        };

        let line = format!("[{}] [{direction}] {summary}", self.timestamp());
        self.append_line(&line);
    }

    /// Record a latency sample (call after processing a message round-trip).
    pub fn record_latency(&self, latency: std::time::Duration) {
        if !self.enabled {
            return;
        }
        let us = latency.as_micros() as u64;
        self.latency_sum_us.fetch_add(us, Ordering::Relaxed);
        self.latency_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Log a connection lifecycle event (connect, disconnect, reconnect, error).
    pub fn log_connection_event(&self, event: &str) {
        if !self.enabled {
            return;
        }
        self.connection_events.fetch_add(1, Ordering::Relaxed);
        let line = format!("[{}] [CONN] {event}", self.timestamp());
        self.append_line(&line);
    }

    /// Log an error event.
    pub fn log_error(&self, error: &str) {
        if !self.enabled {
            return;
        }
        self.errors.fetch_add(1, Ordering::Relaxed);
        let line = format!("[{}] [ERROR] {error}", self.timestamp());
        self.append_line(&line);
    }

    /// Get aggregate stats.
    pub fn get_stats(&self) -> BridgeDebugStats {
        let count = self.latency_count.load(Ordering::Relaxed);
        let sum = self.latency_sum_us.load(Ordering::Relaxed);
        BridgeDebugStats {
            messages_sent: self.sent.load(Ordering::Relaxed),
            messages_received: self.received.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            connection_events: self.connection_events.load(Ordering::Relaxed),
            avg_latency_us: if count > 0 { sum / count } else { 0 },
        }
    }

    /// Uptime since logger creation.
    pub fn uptime(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    /// Whether debug logging is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    // -- Internal --

    fn timestamp(&self) -> String {
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    fn append_line(&self, line: &str) {
        self.maybe_rotate();

        if let Some(parent) = self.log_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        else {
            return;
        };

        let _ = writeln!(file, "{line}");
    }

    /// Rotate log file if it exceeds `MAX_LOG_SIZE`.
    fn maybe_rotate(&self) {
        let Ok(meta) = fs::metadata(&self.log_path) else {
            return; // File doesn't exist yet, nothing to rotate.
        };

        if meta.len() < MAX_LOG_SIZE {
            return;
        }

        // Shift existing rotated files: .3 → delete, .2 → .3, .1 → .2.
        for i in (1..MAX_ROTATED_FILES).rev() {
            let from = rotated_path(&self.log_path, i);
            let to = rotated_path(&self.log_path, i + 1);
            let _ = fs::rename(&from, &to);
        }

        // Current → .1.
        let first_rotated = rotated_path(&self.log_path, 1);
        let _ = fs::rename(&self.log_path, &first_rotated);

        // Create fresh empty file.
        let _ = File::create(&self.log_path);
    }
}

/// Build rotated file path: `bridge-debug.log` → `bridge-debug.log.1`.
fn rotated_path(base: &Path, n: usize) -> PathBuf {
    let mut p = base.as_os_str().to_os_string();
    p.push(format!(".{n}"));
    PathBuf::from(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disabled_logger_is_noop() {
        let logger = BridgeDebugLogger::disabled();
        assert!(!logger.is_enabled());
        logger.log_message(Direction::Send, "test");
        logger.log_connection_event("connect");
        logger.log_error("oops");
        let stats = logger.get_stats();
        assert_eq!(stats.messages_sent, 0);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn test_stats_tracking() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let logger = BridgeDebugLogger::new(tmp.path().to_path_buf(), true);

        logger.log_message(Direction::Send, "msg1");
        logger.log_message(Direction::Send, "msg2");
        logger.log_message(Direction::Recv, "msg3");
        logger.log_error("fail");
        logger.log_connection_event("connect");
        logger.record_latency(std::time::Duration::from_micros(100));
        logger.record_latency(std::time::Duration::from_micros(200));

        let stats = logger.get_stats();
        assert_eq!(stats.messages_sent, 2);
        assert_eq!(stats.messages_received, 1);
        assert_eq!(stats.errors, 1);
        assert_eq!(stats.connection_events, 1);
        assert_eq!(stats.avg_latency_us, 150);
    }

    #[test]
    fn test_log_writes_to_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let logger = BridgeDebugLogger::new(tmp.path().to_path_buf(), true);

        logger.log_message(Direction::Send, "hello bridge");
        logger.log_connection_event("connected to wss://example.com");

        let content = fs::read_to_string(tmp.path()).unwrap();
        assert!(content.contains("[SEND]"));
        assert!(content.contains("hello bridge"));
        assert!(content.contains("[CONN]"));
        assert!(content.contains("connected to wss://example.com"));
    }

    #[test]
    fn test_long_message_truncated() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let logger = BridgeDebugLogger::new(tmp.path().to_path_buf(), true);

        let long_msg = "x".repeat(1000);
        logger.log_message(Direction::Recv, &long_msg);

        let content = fs::read_to_string(tmp.path()).unwrap();
        assert!(content.contains("1000 bytes"));
    }

    #[test]
    fn test_direction_display() {
        assert_eq!(Direction::Send.to_string(), "SEND");
        assert_eq!(Direction::Recv.to_string(), "RECV");
    }

    #[test]
    fn test_uptime() {
        let logger = BridgeDebugLogger::disabled();
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(logger.uptime() >= std::time::Duration::from_millis(10));
    }

    #[test]
    fn test_rotated_path() {
        let base = PathBuf::from("/tmp/bridge-debug.log");
        assert_eq!(rotated_path(&base, 1), PathBuf::from("/tmp/bridge-debug.log.1"));
        assert_eq!(rotated_path(&base, 3), PathBuf::from("/tmp/bridge-debug.log.3"));
    }
}
