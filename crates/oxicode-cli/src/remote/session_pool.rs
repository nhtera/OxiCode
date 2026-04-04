//! Session pool — manages multiple concurrent bridge sessions with lifecycle.
//!
//! Each session has its own ID, creation time, and last-active timestamp.
//! Sessions are cleaned up after idle timeout.

use std::collections::HashMap;
use std::time::Instant;

/// Unique session identifier.
pub type SessionId = String;

/// A single bridge session.
#[derive(Debug)]
pub struct BridgeSession {
    /// Unique session ID.
    pub id: SessionId,
    /// When the session was created.
    pub created_at: Instant,
    /// When the session was last active.
    pub last_active: Instant,
    /// Session metadata (model, user info, etc.).
    pub metadata: SessionMetadata,
}

/// Session metadata — user and configuration info.
#[derive(Debug, Clone, Default)]
pub struct SessionMetadata {
    /// Authenticated user identifier (from JWT).
    pub user_id: Option<String>,
    /// Model requested for this session.
    pub model: String,
    /// Working directory for the session.
    pub working_dir: Option<String>,
}

impl BridgeSession {
    /// Create a new session with the given ID and metadata.
    pub fn new(id: SessionId, metadata: SessionMetadata) -> Self {
        let now = Instant::now();
        Self {
            id,
            created_at: now,
            last_active: now,
            metadata,
        }
    }

    /// Update the last-active timestamp.
    pub fn touch(&mut self) {
        self.last_active = Instant::now();
    }

    /// Seconds since the session was last active.
    pub fn idle_secs(&self) -> u64 {
        self.last_active.elapsed().as_secs()
    }
}

/// Pool of active bridge sessions with capacity limits and idle cleanup.
#[derive(Debug)]
pub struct SessionPool {
    sessions: HashMap<SessionId, BridgeSession>,
    max_sessions: usize,
    idle_timeout_secs: u64,
}

impl SessionPool {
    /// Create a new session pool with capacity and idle timeout.
    pub fn new(max_sessions: usize, idle_timeout_secs: u64) -> Self {
        Self {
            sessions: HashMap::new(),
            max_sessions,
            idle_timeout_secs,
        }
    }

    /// Create a new session. Returns the session ID or an error if at capacity.
    pub fn create_session(&mut self, metadata: SessionMetadata) -> Result<SessionId, String> {
        // Clean up expired sessions first.
        self.cleanup_idle();

        if self.sessions.len() >= self.max_sessions {
            return Err(format!(
                "Session limit reached ({}/{}). Close existing sessions first.",
                self.sessions.len(),
                self.max_sessions
            ));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let session = BridgeSession::new(id.clone(), metadata);
        self.sessions.insert(id.clone(), session);

        tracing::info!(session_id = %id, "Bridge session created");
        Ok(id)
    }

    /// Get a mutable reference to a session (also touches it).
    pub fn get_mut(&mut self, id: &str) -> Option<&mut BridgeSession> {
        if let Some(session) = self.sessions.get_mut(id) {
            session.touch();
            Some(session)
        } else {
            None
        }
    }

    /// Get a read-only reference to a session.
    pub fn get(&self, id: &str) -> Option<&BridgeSession> {
        self.sessions.get(id)
    }

    /// Remove a session by ID.
    pub fn remove(&mut self, id: &str) -> bool {
        let removed = self.sessions.remove(id).is_some();
        if removed {
            tracing::info!(session_id = %id, "Bridge session removed");
        }
        removed
    }

    /// Number of active sessions.
    pub fn active_count(&self) -> usize {
        self.sessions.len()
    }

    /// List all active session IDs.
    pub fn list_session_ids(&self) -> Vec<&str> {
        self.sessions.keys().map(String::as_str).collect()
    }

    /// Remove sessions that have been idle longer than the timeout.
    pub fn cleanup_idle(&mut self) -> usize {
        let timeout = self.idle_timeout_secs;
        let before = self.sessions.len();
        self.sessions.retain(|id, session| {
            let keep = session.idle_secs() < timeout;
            if !keep {
                tracing::info!(
                    session_id = %id,
                    idle_secs = session.idle_secs(),
                    "Session expired (idle timeout)"
                );
            }
            keep
        });
        before - self.sessions.len()
    }

    /// Whether the pool is at capacity.
    pub fn is_full(&self) -> bool {
        self.sessions.len() >= self.max_sessions
    }

    /// Summary string for status display.
    pub fn summary(&self) -> String {
        format!(
            "Sessions: {}/{} (idle timeout: {}s)",
            self.sessions.len(),
            self.max_sessions,
            self.idle_timeout_secs
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_metadata() -> SessionMetadata {
        SessionMetadata {
            user_id: Some("test-user".to_string()),
            model: "claude-sonnet-4-20250514".to_string(),
            working_dir: None,
        }
    }

    #[test]
    fn test_create_session() {
        let mut pool = SessionPool::new(10, 1800);
        let id = pool.create_session(default_metadata()).unwrap();
        assert!(!id.is_empty());
        assert_eq!(pool.active_count(), 1);
    }

    #[test]
    fn test_session_capacity_limit() {
        let mut pool = SessionPool::new(2, 1800);
        pool.create_session(default_metadata()).unwrap();
        pool.create_session(default_metadata()).unwrap();
        let result = pool.create_session(default_metadata());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Session limit"));
    }

    #[test]
    fn test_get_and_touch() {
        let mut pool = SessionPool::new(10, 1800);
        let id = pool.create_session(default_metadata()).unwrap();

        let session = pool.get_mut(&id).unwrap();
        assert_eq!(session.metadata.user_id.as_deref(), Some("test-user"));
    }

    #[test]
    fn test_get_nonexistent() {
        let pool = SessionPool::new(10, 1800);
        assert!(pool.get("nonexistent").is_none());
    }

    #[test]
    fn test_remove_session() {
        let mut pool = SessionPool::new(10, 1800);
        let id = pool.create_session(default_metadata()).unwrap();
        assert!(pool.remove(&id));
        assert_eq!(pool.active_count(), 0);
        assert!(!pool.remove(&id)); // Already removed.
    }

    #[test]
    fn test_list_session_ids() {
        let mut pool = SessionPool::new(10, 1800);
        let id1 = pool.create_session(default_metadata()).unwrap();
        let id2 = pool.create_session(default_metadata()).unwrap();
        let ids = pool.list_session_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1.as_str()));
        assert!(ids.contains(&id2.as_str()));
    }

    #[test]
    fn test_is_full() {
        let mut pool = SessionPool::new(1, 1800);
        assert!(!pool.is_full());
        pool.create_session(default_metadata()).unwrap();
        assert!(pool.is_full());
    }

    #[test]
    fn test_cleanup_idle_no_expired() {
        let mut pool = SessionPool::new(10, 1800);
        pool.create_session(default_metadata()).unwrap();
        let cleaned = pool.cleanup_idle();
        assert_eq!(cleaned, 0);
        assert_eq!(pool.active_count(), 1);
    }

    #[test]
    fn test_cleanup_idle_with_expired() {
        // Use a 0-second timeout so sessions expire immediately.
        let mut pool = SessionPool::new(10, 0);
        pool.create_session(default_metadata()).unwrap();
        // Session should expire on next cleanup.
        let cleaned = pool.cleanup_idle();
        assert_eq!(cleaned, 1);
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn test_summary() {
        let pool = SessionPool::new(10, 1800);
        let summary = pool.summary();
        assert!(summary.contains("0/10"));
        assert!(summary.contains("1800s"));
    }

    #[test]
    fn test_session_idle_secs() {
        let session = BridgeSession::new("test".to_string(), default_metadata());
        // Just created — idle should be near 0.
        assert!(session.idle_secs() < 2);
    }
}
