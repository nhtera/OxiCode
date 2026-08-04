pub mod branch;
pub mod memdir;
pub mod memory;
pub mod memory_extractor;
pub mod memory_freshness;
pub mod memory_scanner;
pub mod memory_selector;
pub mod memory_types;
pub mod prompt_history;

#[cfg(feature = "team_memory_sync")]
pub mod team_memory_sync;

use std::cmp::Reverse;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use oxicode_common::constants::{CONFIG_DIR_NAME, SESSIONS_DIR_NAME};
use oxicode_common::{Message, OxiError, OxiResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A saved conversation session.
///
/// In the legacy flat-file format this maps 1:1 to a JSON file at
/// `sessions/{id}.json`. In the tree format, each branch is stored as a
/// separate file under `sessions/{session_id}/{branch_id}.json`.
/// The `branch_id` / `parent_branch` / `parent_turn` fields are `None` for
/// legacy sessions and populated after migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub messages: Vec<Message>,
    pub model: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// User-assigned display name. None → derive from first user message.
    #[serde(default)]
    pub title: Option<String>,
    /// When part of a `SessionTree`: the branch UUID for this session.
    /// `None` in legacy flat-file sessions.
    #[serde(default)]
    pub branch_id: Option<String>,
    /// When part of a `SessionTree`: parent branch UUID.
    /// `None` for root branches and legacy sessions.
    #[serde(default)]
    pub parent_branch: Option<String>,
    /// When part of a `SessionTree`: message index at which we forked from parent.
    /// `None` for root branches and legacy sessions.
    #[serde(default)]
    pub parent_turn: Option<u32>,
    /// Extra working directories added via `/add-dir` during this session.
    /// Persisted so `--session <id>` restores the set.
    #[serde(default)]
    pub working_dirs: Vec<PathBuf>,
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
            title: None,
            branch_id: None,
            parent_branch: None,
            parent_turn: None,
            working_dirs: Vec::new(),
        }
    }

    pub fn push_message(&mut self, message: Message) {
        self.messages.push(message);
        self.updated_at = Utc::now();
    }

    /// Add a working directory (set semantics — duplicate is a no-op).
    pub fn add_working_dir(&mut self, path: PathBuf) {
        if !self.working_dirs.contains(&path) {
            self.working_dirs.push(path);
            self.updated_at = Utc::now();
        }
    }

    /// Remove a working directory by canonical path. Returns `true` if it was present.
    pub fn remove_working_dir(&mut self, path: &PathBuf) -> bool {
        let before = self.working_dirs.len();
        self.working_dirs.retain(|p| p != path);
        let removed = self.working_dirs.len() < before;
        if removed {
            self.updated_at = Utc::now();
        }
        removed
    }

    /// Replace working_dirs wholesale (used when syncing from `AppState`).
    pub fn set_working_dirs(&mut self, dirs: Vec<PathBuf>) {
        // Deduplicate while preserving insertion order.
        let mut seen: HashSet<PathBuf> = HashSet::new();
        self.working_dirs = dirs
            .into_iter()
            .filter(|p| seen.insert(p.clone()))
            .collect();
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
    /// User-assigned display name (takes precedence over preview when present).
    #[serde(default)]
    pub title: Option<String>,
}

impl SessionSummary {
    /// Best display label: user-assigned title if set, else first user message preview,
    /// else `(empty)`.
    pub fn display_label(&self) -> &str {
        self.title
            .as_deref()
            .or(self.preview.as_deref())
            .unwrap_or("(empty)")
    }

    /// Human-readable one-line display.
    pub fn display(&self) -> String {
        format!(
            "{} [{}] ({} msgs) — {}",
            self.id.chars().take(8).collect::<String>(),
            self.model,
            self.message_count,
            self.display_label(),
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
///
/// Resolution order:
/// 1. `sessions/{id}/{id}.json` — tree root branch (common post-migration case).
/// 2. `sessions/{id}.json`      — legacy flat file (preserved for 1 release cycle).
///
/// No automatic migration is performed here — call
/// [`branch::migrate_legacy_if_needed`] explicitly when you want to migrate a
/// session to tree format.
pub fn load_session(id: &str, config_dir_override: Option<&Path>) -> OxiResult<Session> {
    // Reject path traversal in session IDs.
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(OxiError::Session(format!(
            "Invalid session ID (contains path separators): {id}"
        )));
    }

    let dir = sessions_dir(config_dir_override);

    // 1. Try tree root branch first: sessions/{id}/{id}.json
    let tree_branch_path = dir.join(id).join(format!("{id}.json"));
    if tree_branch_path.exists() {
        let content = fs::read_to_string(&tree_branch_path)?;
        let session: Session = serde_json::from_str(&content)?;
        return Ok(session);
    }

    // 2. Fall back to legacy flat file.
    let legacy_path = dir.join(format!("{id}.json"));
    if !legacy_path.exists() {
        return Err(OxiError::Session(format!("Session not found: {id}")));
    }

    let content = fs::read_to_string(&legacy_path)?;
    Ok(serde_json::from_str(&content)?)
}

/// List all saved sessions (sorted by `updated_at` descending).
///
/// Discovers both:
/// - Tree-layout sessions: `sessions/{session_id}/tree.json` + root branch
/// - Legacy flat-file sessions: `sessions/{id}.json` (only if no tree dir exists)
pub fn list_sessions(config_dir_override: Option<&Path>) -> OxiResult<Vec<SessionSummary>> {
    let dir = sessions_dir(config_dir_override);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut summaries: Vec<SessionSummary> = Vec::new();
    // Track IDs we've already added (prevents duplicates when both tree + legacy exist).
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Tree-layout: read the active branch from tree.json.
            let manifest_path = path.join("tree.json");
            if manifest_path.exists() {
                if let Ok(manifest_content) = fs::read_to_string(&manifest_path) {
                    #[derive(serde::Deserialize)]
                    struct Manifest {
                        current: uuid::Uuid,
                        session_id: uuid::Uuid,
                    }
                    if let Ok(manifest) = serde_json::from_str::<Manifest>(&manifest_content) {
                        let branch_path = path.join(format!("{}.json", manifest.current));
                        if let Ok(content) = fs::read_to_string(&branch_path) {
                            if let Ok(session) = serde_json::from_str::<Session>(&content) {
                                let id = manifest.session_id.to_string();
                                if seen_ids.insert(id.clone()) {
                                    summaries.push(session_to_summary(session, id));
                                }
                            }
                        }
                    }
                }
            }
        } else if path.extension().is_some_and(|ext| ext == "json") {
            // Legacy flat-file: only include when no tree dir exists for same ID.
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            // Skip non-UUID-looking names (e.g. partial writes).
            if !seen_ids.contains(&stem) {
                // Check there's no corresponding tree dir (tree takes precedence).
                let tree_dir_path = dir.join(&stem);
                if !tree_dir_path.exists() {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(session) = serde_json::from_str::<Session>(&content) {
                            if seen_ids.insert(stem.clone()) {
                                summaries.push(session_to_summary(session, stem));
                            }
                        }
                    }
                }
            }
        }
    }

    summaries.sort_by_key(|s| Reverse(s.updated_at));
    Ok(summaries)
}

/// Build a `SessionSummary` from a `Session`, overriding the stored `id` with the
/// caller-supplied `effective_id` (needed for tree sessions where the session_id
/// comes from the manifest, not the branch file's `id` field).
fn session_to_summary(session: Session, effective_id: String) -> SessionSummary {
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

    SessionSummary {
        id: effective_id,
        model: session.model,
        message_count: session.messages.len(),
        created_at: session.created_at,
        updated_at: session.updated_at,
        preview,
        title: session.title,
    }
}

/// Rename a session by updating its `title` and persisting to disk.
/// `new_title` is trimmed; empty values are rejected.
pub fn rename_session(
    id: &str,
    new_title: &str,
    config_dir_override: Option<&Path>,
) -> OxiResult<()> {
    let trimmed = new_title.trim();
    if trimmed.is_empty() {
        return Err(OxiError::Session("Session title cannot be empty".into()));
    }
    let mut session = load_session(id, config_dir_override)?;
    session.title = Some(trimmed.to_string());
    session.updated_at = Utc::now();
    save_session(&session, config_dir_override)?;
    Ok(())
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

    #[test]
    fn test_rename_session_persists_title() {
        let tmp = tempfile::tempdir().unwrap();
        let mut session = Session::new("model");
        session.push_message(Message::user("first"));
        save_session(&session, Some(tmp.path())).unwrap();
        assert!(session.title.is_none());

        rename_session(&session.id, "  my-custom-title  ", Some(tmp.path())).unwrap();

        let reloaded = load_session(&session.id, Some(tmp.path())).unwrap();
        assert_eq!(reloaded.title.as_deref(), Some("my-custom-title"));

        // List surfaces the title too.
        let list = list_sessions(Some(tmp.path())).unwrap();
        assert_eq!(list[0].title.as_deref(), Some("my-custom-title"));
        assert_eq!(list[0].display_label(), "my-custom-title");
    }

    #[test]
    fn test_rename_session_rejects_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let session = Session::new("model");
        save_session(&session, Some(tmp.path())).unwrap();
        assert!(rename_session(&session.id, "   ", Some(tmp.path())).is_err());
    }

    #[test]
    fn test_display_label_falls_back_to_preview() {
        let now = Utc::now();
        let summary = SessionSummary {
            id: "abc".into(),
            model: "m".into(),
            message_count: 1,
            created_at: now,
            updated_at: now,
            preview: Some("first message".into()),
            title: None,
        };
        assert_eq!(summary.display_label(), "first message");
    }

    // ── working_dirs ─────────────────────────────────────────────────────

    #[test]
    fn test_session_working_dirs_default_empty() {
        let session = Session::new("model");
        assert!(session.working_dirs.is_empty());
    }

    #[test]
    fn test_session_add_working_dir_set_semantics() {
        let mut session = Session::new("model");
        let dir = PathBuf::from("/tmp/alpha");
        session.add_working_dir(dir.clone());
        session.add_working_dir(dir.clone()); // duplicate — should be no-op
        assert_eq!(session.working_dirs.len(), 1);
    }

    #[test]
    fn test_session_remove_working_dir() {
        let mut session = Session::new("model");
        let dir = PathBuf::from("/tmp/beta");
        session.add_working_dir(dir.clone());
        assert_eq!(session.working_dirs.len(), 1);

        let removed = session.remove_working_dir(&dir);
        assert!(removed);
        assert!(session.working_dirs.is_empty());

        // Second removal returns false.
        let removed2 = session.remove_working_dir(&dir);
        assert!(!removed2);
    }

    #[test]
    fn test_session_working_dirs_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut session = Session::new("model");
        session.push_message(Message::user("hi"));
        session.add_working_dir(PathBuf::from("/tmp/dir_a"));
        session.add_working_dir(PathBuf::from("/tmp/dir_b"));

        save_session(&session, Some(tmp.path())).unwrap();
        let loaded = load_session(&session.id, Some(tmp.path())).unwrap();

        assert_eq!(loaded.working_dirs.len(), 2);
        assert_eq!(loaded.working_dirs[0], PathBuf::from("/tmp/dir_a"));
        assert_eq!(loaded.working_dirs[1], PathBuf::from("/tmp/dir_b"));
    }

    #[test]
    fn test_session_working_dirs_backward_compat() {
        // A JSON session without `working_dirs` should load with empty vec.
        let tmp = tempfile::tempdir().unwrap();
        let session = Session::new("model");
        // Serialize manually without working_dirs field (simulate legacy).
        let legacy_json = serde_json::json!({
            "id": session.id,
            "messages": [],
            "model": session.model,
            "created_at": session.created_at,
            "updated_at": session.updated_at
        });
        let sessions_dir = crate::sessions_dir(Some(tmp.path()));
        std::fs::create_dir_all(&sessions_dir).unwrap();
        let path = sessions_dir.join(format!("{}.json", session.id));
        std::fs::write(path, serde_json::to_string_pretty(&legacy_json).unwrap()).unwrap();

        let loaded = load_session(&session.id, Some(tmp.path())).unwrap();
        assert!(loaded.working_dirs.is_empty());
    }

    #[test]
    fn test_session_set_working_dirs_deduplicates() {
        let mut session = Session::new("model");
        let dirs = vec![
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/a"), // duplicate
        ];
        session.set_working_dirs(dirs);
        assert_eq!(session.working_dirs.len(), 2);
    }
}
