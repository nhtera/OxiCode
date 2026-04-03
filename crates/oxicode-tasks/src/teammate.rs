//! In-process teammate task — spawns a collaborative agent as a tokio task.
//!
//! Shares state via message passing rather than subprocess spawning,
//! enabling efficient in-process team collaboration with file ownership guards.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use oxicode_common::OxiResult;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};

use crate::manager::TaskStatus;
use crate::task_output_helpers::{open_output_file, write_line};

/// A message passed between the coordinator and a teammate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateMessage {
    pub from: String,
    pub to: String,
    pub content: String,
    pub timestamp: String,
}

/// Tracks which files each teammate "owns" to prevent edit conflicts.
#[derive(Debug, Default)]
pub struct FileOwnershipGuard {
    /// Map of file glob patterns to owner names.
    owned: std::collections::HashMap<String, String>,
}

impl FileOwnershipGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim ownership of a file path for a given teammate.
    /// Returns `Err` if the file is already owned by someone else.
    pub fn claim(&mut self, path: &str, owner: &str) -> Result<(), String> {
        if let Some(existing) = self.owned.get(path) {
            if existing != owner {
                return Err(format!(
                    "file {path} already owned by {existing}, cannot claim for {owner}"
                ));
            }
        }
        self.owned.insert(path.to_string(), owner.to_string());
        Ok(())
    }

    /// Release all files owned by a teammate.
    pub fn release_all(&mut self, owner: &str) {
        self.owned.retain(|_, v| v != owner);
    }

    /// Check if a file is owned by a specific teammate.
    pub fn is_owned_by(&self, path: &str, owner: &str) -> bool {
        self.owned.get(path).is_some_and(|o| o == owner)
    }

    /// List all files owned by a teammate.
    pub fn files_for(&self, owner: &str) -> Vec<&str> {
        self.owned
            .iter()
            .filter(|(_, v)| v.as_str() == owner)
            .map(|(k, _)| k.as_str())
            .collect()
    }
}

/// Simple broadcast message bus for teammate coordination.
#[derive(Debug)]
pub struct MessageBus {
    subscribers: Vec<(String, mpsc::UnboundedSender<TeammateMessage>)>,
}

impl MessageBus {
    pub fn new() -> Self {
        Self {
            subscribers: Vec::new(),
        }
    }

    /// Register a subscriber. Returns a receiver for incoming messages.
    pub fn subscribe(&mut self, name: &str) -> mpsc::UnboundedReceiver<TeammateMessage> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.subscribers.push((name.to_string(), tx));
        rx
    }

    /// Send a message to a specific teammate, or broadcast to all if `to` is "*".
    pub fn send(&self, msg: TeammateMessage) {
        let is_broadcast = msg.to == "*";
        for (name, tx) in &self.subscribers {
            if is_broadcast || name == &msg.to {
                let _ = tx.send(msg.clone());
            }
        }
    }

    /// Remove a subscriber by name (cleanup after task ends).
    pub fn unsubscribe(&mut self, name: &str) {
        self.subscribers.retain(|(n, _)| n != name);
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawn an in-process teammate as a tokio task.
///
/// The teammate receives its prompt, processes it, and writes output to disk.
/// File ownership is tracked via the shared `FileOwnershipGuard`.
/// Communication happens through the `MessageBus`.
pub async fn run_teammate(
    task_id: &str,
    teammate_name: &str,
    prompt: &str,
    owned_files: &[String],
    file_guard: Arc<Mutex<FileOwnershipGuard>>,
    message_bus: Arc<Mutex<MessageBus>>,
    tasks_dir: &Path,
) -> OxiResult<TaskStatus> {
    tracing::info!(
        "run_teammate task={} name={}",
        task_id,
        teammate_name
    );

    let mut out_file = open_output_file(tasks_dir, task_id)?;

    // Claim file ownership.
    {
        let mut guard = file_guard.lock().await;
        for file_path in owned_files {
            if let Err(e) = guard.claim(file_path, teammate_name) {
                let error = format!("ownership conflict: {e}");
                write_line(&mut out_file, "stderr", &error)?;
                return Ok(TaskStatus::Failed { error });
            }
        }
    }

    // Subscribe to message bus.
    let mut rx = {
        let mut bus = message_bus.lock().await;
        bus.subscribe(teammate_name)
    };

    write_line(
        &mut out_file,
        "stdout",
        &format!("teammate {teammate_name} started with prompt: {prompt}"),
    )?;

    // Track received messages for the teammate's context.
    let mut received: HashSet<String> = HashSet::new();

    // Process incoming messages with a timeout.
    let timeout = tokio::time::Duration::from_secs(5);
    loop {
        match tokio::time::timeout(timeout, rx.recv()).await {
            Ok(Some(msg)) => {
                let key = format!("{}:{}", msg.from, msg.content);
                if received.insert(key) {
                    write_line(
                        &mut out_file,
                        "stdout",
                        &format!("[msg from {}] {}", msg.from, msg.content),
                    )?;
                }
            }
            Ok(None) => {
                // Channel closed — bus dropped or unsubscribed.
                break;
            }
            Err(_) => {
                // Timeout — no more messages, teammate work is done.
                break;
            }
        }
    }

    write_line(
        &mut out_file,
        "stdout",
        &format!("teammate {teammate_name} completed"),
    )?;

    // Release file ownership on completion.
    {
        let mut guard = file_guard.lock().await;
        guard.release_all(teammate_name);
    }

    // Unsubscribe from message bus.
    {
        let mut bus = message_bus.lock().await;
        bus.unsubscribe(teammate_name);
    }

    tracing::info!("run_teammate task={} completed", task_id);
    Ok(TaskStatus::Completed { exit_code: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn tmp_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("oxi-teammate-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn file_ownership_claim_and_release() {
        let mut guard = FileOwnershipGuard::new();
        assert!(guard.claim("src/main.rs", "alice").is_ok());
        assert!(guard.is_owned_by("src/main.rs", "alice"));

        // Another teammate cannot claim the same file.
        assert!(guard.claim("src/main.rs", "bob").is_err());

        // Same teammate can re-claim.
        assert!(guard.claim("src/main.rs", "alice").is_ok());

        guard.release_all("alice");
        assert!(!guard.is_owned_by("src/main.rs", "alice"));

        // Now bob can claim it.
        assert!(guard.claim("src/main.rs", "bob").is_ok());
    }

    #[test]
    fn file_ownership_list_files() {
        let mut guard = FileOwnershipGuard::new();
        guard.claim("a.rs", "alice").unwrap();
        guard.claim("b.rs", "alice").unwrap();
        guard.claim("c.rs", "bob").unwrap();
        let alice_files = guard.files_for("alice");
        assert_eq!(alice_files.len(), 2);
        assert!(alice_files.contains(&"a.rs"));
        assert!(alice_files.contains(&"b.rs"));
    }

    #[test]
    fn message_bus_send_and_receive() {
        let mut bus = MessageBus::new();
        let mut rx = bus.subscribe("alice");

        bus.send(TeammateMessage {
            from: "bob".into(),
            to: "alice".into(),
            content: "hello alice".into(),
            timestamp: Utc::now().to_rfc3339(),
        });

        let msg = rx.try_recv().unwrap();
        assert_eq!(msg.content, "hello alice");
    }

    #[test]
    fn message_bus_broadcast() {
        let mut bus = MessageBus::new();
        let mut rx_alice = bus.subscribe("alice");
        let mut rx_bob = bus.subscribe("bob");

        bus.send(TeammateMessage {
            from: "lead".into(),
            to: "*".into(),
            content: "all hands".into(),
            timestamp: Utc::now().to_rfc3339(),
        });

        assert!(rx_alice.try_recv().is_ok());
        assert!(rx_bob.try_recv().is_ok());
    }

    #[tokio::test]
    async fn run_teammate_basic_lifecycle() {
        let dir = tmp_dir();
        let guard = Arc::new(Mutex::new(FileOwnershipGuard::new()));
        let bus = Arc::new(Mutex::new(MessageBus::new()));

        let status = run_teammate(
            "t-mate-1",
            "tester",
            "run tests",
            &["tests/".to_string()],
            guard.clone(),
            bus.clone(),
            &dir,
        )
        .await
        .unwrap();

        assert!(matches!(status, TaskStatus::Completed { .. }));
        let output = std::fs::read_to_string(dir.join("t-mate-1/output.jsonl")).unwrap();
        assert!(output.contains("tester started"));
        assert!(output.contains("tester completed"));
    }

    #[tokio::test]
    async fn run_teammate_ownership_conflict() {
        let dir = tmp_dir();
        let guard = Arc::new(Mutex::new(FileOwnershipGuard::new()));
        let bus = Arc::new(Mutex::new(MessageBus::new()));

        // Pre-claim a file for someone else.
        guard.lock().await.claim("src/main.rs", "alice").unwrap();

        let status = run_teammate(
            "t-mate-2",
            "bob",
            "edit main",
            &["src/main.rs".to_string()],
            guard.clone(),
            bus.clone(),
            &dir,
        )
        .await
        .unwrap();

        assert!(
            matches!(status, TaskStatus::Failed { .. }),
            "should fail on ownership conflict"
        );
    }
}
