//! Session bridge: wraps existing session management for IDE bridge protocol.
//!
//! Provides create/resume/list operations through the bridge layer, translating
//! between bridge protocol types and the internal session store.

use std::collections::HashMap;
use std::sync::Arc;

use oxicode_common::PermissionResponse;
use oxicode_core::Conversation;
use oxicode_session::Session;
use tokio::sync::{oneshot, Mutex};

/// Shared map of pending permission reply channels.
pub type PermMap = Arc<Mutex<HashMap<String, oneshot::Sender<PermissionResponse>>>>;

/// Per-session state managed by the bridge.
pub struct BridgeSessionState {
    pub session: Session,
    pub conversation: Conversation,
    /// Cancel token for the active turn (if any).
    pub cancel_tx: Option<oneshot::Sender<()>>,
    /// Pending permission reply channels keyed by permission_id.
    pub active_perms: PermMap,
    /// Whether a turn is currently executing.
    pub is_streaming: bool,
}

impl BridgeSessionState {
    /// Create a new bridge session state from a fresh session.
    pub fn new(session: Session) -> Self {
        Self {
            session,
            conversation: Conversation::new(),
            cancel_tx: None,
            active_perms: Arc::new(Mutex::new(HashMap::new())),
            is_streaming: false,
        }
    }

    /// Create from an existing session (resume), populating conversation.
    pub fn from_existing(session: Session) -> Self {
        let mut conversation = Conversation::new();
        for msg in &session.messages {
            conversation.push(msg.clone());
        }
        Self {
            session,
            conversation,
            cancel_tx: None,
            active_perms: Arc::new(Mutex::new(HashMap::new())),
            is_streaming: false,
        }
    }

    /// Current state label for status queries.
    pub fn state_label(&self) -> &'static str {
        if self.is_streaming {
            "streaming"
        } else if !self.active_perms.try_lock().is_ok_and(|p| p.is_empty()) {
            // If lock contended (map_or false → !false = true) → conservatively report awaiting.
            // If lock acquired and non-empty → also report awaiting.
            "awaiting_permission"
        } else {
            "idle"
        }
    }
}

/// Manages all bridge sessions with thread-safe access.
pub struct SessionBridge {
    sessions: Mutex<HashMap<String, BridgeSessionState>>,
}

impl SessionBridge {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Create a new session, returning its ID.
    pub async fn create_session(&self, model: &str) -> String {
        let session = Session::new(model);
        let session_id = session.id.clone();
        let state = BridgeSessionState::new(session);
        self.sessions.lock().await.insert(session_id.clone(), state);
        session_id
    }

    /// Resume an existing session from disk.
    pub async fn resume_session(&self, session_id: &str) -> Result<usize, String> {
        // Already loaded?
        {
            let sessions = self.sessions.lock().await;
            if let Some(s) = sessions.get(session_id) {
                return Ok(s.session.messages.len());
            }
        }

        // Load from disk.
        let session = oxicode_session::load_session(session_id, None)
            .map_err(|e| format!("Session not found: {e}"))?;
        let msg_count = session.messages.len();
        let state = BridgeSessionState::from_existing(session);
        self.sessions
            .lock()
            .await
            .insert(session_id.to_string(), state);
        Ok(msg_count)
    }

    /// List all loaded sessions as (id, message_count, model) tuples.
    pub async fn list_sessions(&self) -> Vec<(String, usize, String)> {
        let sessions = self.sessions.lock().await;
        sessions
            .values()
            .map(|s| {
                (
                    s.session.id.clone(),
                    s.session.messages.len(),
                    s.session.model.clone(),
                )
            })
            .collect()
    }

    /// Access the sessions map for handler operations.
    pub fn sessions(&self) -> &Mutex<HashMap<String, BridgeSessionState>> {
        &self.sessions
    }

    /// Save all sessions to disk (used during shutdown).
    pub async fn save_all(&self) {
        let sessions = self.sessions.lock().await;
        for state in sessions.values() {
            let _ = oxicode_session::save_session(&state.session, None);
        }
    }

    /// Cancel all active turns and save sessions.
    pub async fn shutdown(&self) {
        let mut sessions = self.sessions.lock().await;
        for state in sessions.values_mut() {
            if let Some(cancel_tx) = state.cancel_tx.take() {
                let _ = cancel_tx.send(());
            }
            let _ = oxicode_session::save_session(&state.session, None);
        }
    }
}

impl Default for SessionBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_session_returns_id() {
        let bridge = SessionBridge::new();
        let id = bridge.create_session("claude-sonnet-4-20250514").await;
        assert!(!id.is_empty());
    }

    #[tokio::test]
    async fn list_sessions_includes_created() {
        let bridge = SessionBridge::new();
        let id = bridge.create_session("test-model").await;
        let list = bridge.list_sessions().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, id);
        assert_eq!(list[0].2, "test-model");
    }

    #[tokio::test]
    async fn create_multiple_sessions() {
        let bridge = SessionBridge::new();
        let _id1 = bridge.create_session("model-a").await;
        let _id2 = bridge.create_session("model-b").await;
        let list = bridge.list_sessions().await;
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn resume_nonexistent_session_errors() {
        let bridge = SessionBridge::new();
        let result = bridge.resume_session("nonexistent-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn resume_already_loaded_returns_count() {
        let bridge = SessionBridge::new();
        let id = bridge.create_session("model").await;
        let result = bridge.resume_session(&id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0); // fresh session has 0 messages
    }

    #[test]
    fn state_label_idle_by_default() {
        let session = Session::new("model");
        let state = BridgeSessionState::new(session);
        assert_eq!(state.state_label(), "idle");
    }

    #[test]
    fn state_label_streaming() {
        let session = Session::new("model");
        let mut state = BridgeSessionState::new(session);
        state.is_streaming = true;
        assert_eq!(state.state_label(), "streaming");
    }

    #[tokio::test]
    async fn shutdown_cancels_and_saves() {
        let bridge = SessionBridge::new();
        let _id = bridge.create_session("model").await;
        // Just verify it doesn't panic.
        bridge.shutdown().await;
    }

    #[test]
    fn session_bridge_default() {
        let bridge = SessionBridge::default();
        // Verify sessions map is accessible.
        let _sessions = bridge.sessions();
    }
}
