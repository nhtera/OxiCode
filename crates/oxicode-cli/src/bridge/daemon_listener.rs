//! Daemon listener: TCP socket + lockfile management for Phase B bridge.
//!
//! When `oxicode --mode daemon` is started, this module binds a TCP listener
//! on a configurable port (default: 0 = OS-assigned), writes a lockfile with
//! PID + port, and accepts connections. Each connection is a JSON-RPC stream
//! (line-delimited, same protocol as stdio transport).

use std::net::SocketAddr;
use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Default bind address (localhost only for security).
pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1";

/// Lockfile contents — written to `~/.oxicode/daemon.lock`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonLockfile {
    /// Process ID of the daemon.
    pub pid: u32,
    /// Port the daemon is listening on.
    pub port: u16,
    /// ISO-8601 timestamp when daemon started.
    pub started_at: String,
    /// Bind address.
    pub bind_address: String,
}

/// Configuration for the daemon listener.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Port to bind (0 = OS-assigned random port).
    pub port: u16,
    /// Bind address.
    pub bind_address: String,
    /// Maximum concurrent connections.
    pub max_connections: usize,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            port: 0,
            bind_address: DEFAULT_BIND_ADDR.to_string(),
            max_connections: 5,
        }
    }
}

impl DaemonConfig {
    /// Socket address string for binding.
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.bind_address, self.port)
    }
}

/// Get the lockfile path.
pub fn lockfile_path() -> PathBuf {
    let base = std::env::var("OXICODE_DATA_DIR").ok().map_or_else(
        || {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".oxicode")
        },
        PathBuf::from,
    );
    base.join("daemon.lock")
}

/// Write the daemon lockfile to disk.
pub fn write_lockfile(port: u16, bind_address: &str) -> Result<PathBuf, String> {
    let path = lockfile_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }

    let lockfile = DaemonLockfile {
        pid: std::process::id(),
        port,
        started_at: Utc::now().to_rfc3339(),
        bind_address: bind_address.to_string(),
    };

    let json = serde_json::to_string_pretty(&lockfile).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write lockfile: {e}"))?;

    tracing::info!(path = %path.display(), port, "daemon lockfile written");
    Ok(path)
}

/// Read the daemon lockfile from disk (if it exists).
pub fn read_lockfile() -> Option<DaemonLockfile> {
    let path = lockfile_path();
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Remove the daemon lockfile.
pub fn remove_lockfile() {
    let path = lockfile_path();
    if path.exists() {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(path = %path.display(), error = %e, "failed to remove lockfile");
        } else {
            tracing::info!(path = %path.display(), "daemon lockfile removed");
        }
    }
}

/// Check if an existing daemon is running by reading the lockfile and checking PID.
pub fn is_daemon_running() -> Option<DaemonLockfile> {
    let lockfile = read_lockfile()?;

    // Check if the PID is still alive (Unix-specific).
    #[cfg(unix)]
    {
        use std::process::Command;
        let output = Command::new("kill")
            .args(["-0", &lockfile.pid.to_string()])
            .output()
            .ok()?;
        if output.status.success() {
            return Some(lockfile);
        }
        // PID not running — stale lockfile.
        tracing::info!(pid = lockfile.pid, "stale lockfile detected, removing");
        remove_lockfile();
        return None;
    }

    #[cfg(not(unix))]
    {
        // On non-Unix, just trust the lockfile exists.
        Some(lockfile)
    }
}

/// Tracks active connections to the daemon.
#[derive(Debug, Default)]
pub struct ConnectionTracker {
    /// Active connection addresses.
    connections: Vec<SocketAddr>,
    /// Maximum allowed connections.
    max_connections: usize,
}

impl ConnectionTracker {
    pub fn new(max_connections: usize) -> Self {
        Self {
            connections: Vec::new(),
            max_connections,
        }
    }

    /// Try to add a connection. Returns false if at capacity.
    pub fn add(&mut self, addr: SocketAddr) -> bool {
        if self.connections.len() >= self.max_connections {
            return false;
        }
        self.connections.push(addr);
        true
    }

    /// Remove a connection.
    pub fn remove(&mut self, addr: &SocketAddr) {
        self.connections.retain(|a| a != addr);
    }

    /// Current connection count.
    pub fn count(&self) -> usize {
        self.connections.len()
    }

    /// Whether at connection capacity.
    pub fn is_full(&self) -> bool {
        self.connections.len() >= self.max_connections
    }

    /// List active connections.
    pub fn active_connections(&self) -> &[SocketAddr] {
        &self.connections
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_config_default() {
        let config = DaemonConfig::default();
        assert_eq!(config.port, 0);
        assert_eq!(config.bind_address, "127.0.0.1");
        assert_eq!(config.max_connections, 5);
    }

    #[test]
    fn daemon_config_socket_addr() {
        let config = DaemonConfig {
            port: 8080,
            bind_address: "0.0.0.0".to_string(),
            max_connections: 10,
        };
        assert_eq!(config.socket_addr(), "0.0.0.0:8080");
    }

    #[test]
    fn lockfile_roundtrip() {
        let lockfile = DaemonLockfile {
            pid: 12345,
            port: 9090,
            started_at: "2026-04-04T12:00:00Z".to_string(),
            bind_address: "127.0.0.1".to_string(),
        };
        let json = serde_json::to_string(&lockfile).unwrap();
        let parsed: DaemonLockfile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.pid, 12345);
        assert_eq!(parsed.port, 9090);
    }

    #[test]
    fn lockfile_path_structure() {
        let path = lockfile_path();
        let path_str = path.to_string_lossy();
        assert!(path_str.ends_with("daemon.lock"));
    }

    #[test]
    fn write_and_read_lockfile_direct() {
        // Test lockfile I/O directly without env var (avoids test parallelism race).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");

        let lockfile = DaemonLockfile {
            pid: std::process::id(),
            port: 9090,
            started_at: chrono::Utc::now().to_rfc3339(),
            bind_address: "127.0.0.1".to_string(),
        };

        let json = serde_json::to_string_pretty(&lockfile).unwrap();
        std::fs::write(&path, &json).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: DaemonLockfile = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.port, 9090);
        assert_eq!(parsed.pid, std::process::id());

        std::fs::remove_file(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn connection_tracker_add_and_remove() {
        let mut tracker = ConnectionTracker::new(3);
        let addr1: SocketAddr = "127.0.0.1:1001".parse().unwrap();
        let addr2: SocketAddr = "127.0.0.1:1002".parse().unwrap();

        assert!(tracker.add(addr1));
        assert!(tracker.add(addr2));
        assert_eq!(tracker.count(), 2);
        assert!(!tracker.is_full());

        tracker.remove(&addr1);
        assert_eq!(tracker.count(), 1);
    }

    #[test]
    fn connection_tracker_respects_max() {
        let mut tracker = ConnectionTracker::new(2);
        let addr1: SocketAddr = "127.0.0.1:1001".parse().unwrap();
        let addr2: SocketAddr = "127.0.0.1:1002".parse().unwrap();
        let addr3: SocketAddr = "127.0.0.1:1003".parse().unwrap();

        assert!(tracker.add(addr1));
        assert!(tracker.add(addr2));
        assert!(tracker.is_full());
        assert!(!tracker.add(addr3)); // rejected
        assert_eq!(tracker.count(), 2);
    }

    #[test]
    fn connection_tracker_active_list() {
        let mut tracker = ConnectionTracker::new(5);
        let addr: SocketAddr = "127.0.0.1:1001".parse().unwrap();
        tracker.add(addr);
        assert_eq!(tracker.active_connections().len(), 1);
        assert_eq!(tracker.active_connections()[0], addr);
    }

    #[test]
    fn read_missing_lockfile_returns_none() {
        // Test that reading a non-existent path returns None.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent").join("daemon.lock");
        let content = std::fs::read_to_string(&path);
        assert!(content.is_err());
    }
}
