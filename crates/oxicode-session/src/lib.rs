pub mod memory;

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use oxicode_common::constants::{CONFIG_DIR_NAME, SESSIONS_DIR_NAME};
use oxicode_common::{Message, OxiError, OxiResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A saved conversation session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub messages: Vec<Message>,
    pub model: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Session {
    pub fn new(model: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            messages: Vec::new(),
            model: model.into(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn push_message(&mut self, message: Message) {
        self.messages.push(message);
        self.updated_at = Utc::now();
    }
}

/// Summary of a session (for listing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub model: String,
    pub message_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// First user message preview.
    pub preview: Option<String>,
}

impl SessionSummary {
    /// Human-readable one-line display.
    pub fn display(&self) -> String {
        let preview = self.preview.as_deref().unwrap_or("(empty)");
        format!(
            "{} [{}] ({} msgs) — {}",
            self.id.chars().take(8).collect::<String>(),
            self.model,
            self.message_count,
            preview,
        )
    }
}

/// Get the sessions directory path.
pub fn sessions_dir(config_dir_override: Option<&Path>) -> PathBuf {
    let base = config_dir_override.map_or_else(
        || {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(CONFIG_DIR_NAME)
        },
        PathBuf::from,
    );
    base.join(SESSIONS_DIR_NAME)
}

/// Save a session to disk as JSON.
pub fn save_session(session: &Session, config_dir_override: Option<&Path>) -> OxiResult<PathBuf> {
    let dir = sessions_dir(config_dir_override);
    fs::create_dir_all(&dir)
        .map_err(|e| OxiError::Session(format!("Failed to create sessions dir: {e}")))?;

    let path = dir.join(format!("{}.json", session.id));
    let json = serde_json::to_string_pretty(session)?;

    // H1 FIX: Write file with restricted permissions on Unix (0o600).
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder
            .create(&dir)
            .map_err(|e| OxiError::Session(format!("Failed to set dir permissions: {e}")))?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(json.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        fs::write(&path, json)?;
    }
    tracing::debug!("Session saved to {}", path.display());
    Ok(path)
}

/// Load a session from disk by ID.
pub fn load_session(id: &str, config_dir_override: Option<&Path>) -> OxiResult<Session> {
    // Reject path traversal in session IDs
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(OxiError::Session(format!(
            "Invalid session ID (contains path separators): {id}"
        )));
    }

    let dir = sessions_dir(config_dir_override);
    let path = dir.join(format!("{id}.json"));

    if !path.exists() {
        return Err(OxiError::Session(format!("Session not found: {id}")));
    }

    let content = fs::read_to_string(&path)?;
    let session: Session = serde_json::from_str(&content)?;
    Ok(session)
}

/// List all saved sessions (sorted by `updated_at` descending).
pub fn list_sessions(config_dir_override: Option<&Path>) -> OxiResult<Vec<SessionSummary>> {
    let dir = sessions_dir(config_dir_override);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut summaries = Vec::new();

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(session) = serde_json::from_str::<Session>(&content) {
                    // H4 FIX: Use chars().take() to avoid byte-slicing panics on UTF-8.
                    let preview = session
                        .messages
                        .iter()
                        .find(|m| m.role == oxicode_common::Role::User)
                        .map(|m| {
                            let text = m.text();
                            if text.chars().count() > 80 {
                                let truncated: String = text.chars().take(77).collect();
                                format!("{truncated}...")
                            } else {
                                text
                            }
                        });

                    summaries.push(SessionSummary {
                        id: session.id,
                        model: session.model,
                        message_count: session.messages.len(),
                        created_at: session.created_at,
                        updated_at: session.updated_at,
                        preview,
                    });
                }
            }
        }
    }

    summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_save_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut session = Session::new("claude-sonnet-4-20250514");
        session.push_message(Message::user("hello world"));

        let path = save_session(&session, Some(tmp.path())).unwrap();
        assert!(path.exists());

        let loaded = load_session(&session.id, Some(tmp.path())).unwrap();
        assert_eq!(loaded.id, session.id);
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].text(), "hello world");
    }

    #[test]
    fn test_list_sessions() {
        let tmp = tempfile::tempdir().unwrap();

        let mut s1 = Session::new("model-a");
        s1.push_message(Message::user("first"));
        save_session(&s1, Some(tmp.path())).unwrap();

        let mut s2 = Session::new("model-b");
        s2.push_message(Message::user("second"));
        save_session(&s2, Some(tmp.path())).unwrap();

        let list = list_sessions(Some(tmp.path())).unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_load_nonexistent_session() {
        let tmp = tempfile::tempdir().unwrap();
        let result = load_session("nonexistent", Some(tmp.path()));
        assert!(result.is_err());
    }
}
