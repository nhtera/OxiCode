//! Session branching: tree-structured conversation sessions.
//!
//! A `SessionTree` holds one root branch plus any number of child branches,
//! all sharing the same `session_id`. Branches are stored on disk as individual
//! JSON files under `~/.oxicode/sessions/{session_id}/{branch_id}.json` plus a
//! `tree.json` manifest that records the active branch pointer.
//!
//! # Storage layout
//! ```text
//! ~/.oxicode/sessions/
//! ├── {session_id}/               # tree dir
//! │   ├── tree.json               # manifest: session_id + current branch
//! │   ├── {branch_id_root}.json   # root branch
//! │   ├── {branch_id_A}.json      # child branch A
//! │   └── {branch_id_B}.json      # child branch B
//! └── {legacy_id}.json            # pre-migration flat file (read-only compat)
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use oxicode_common::{Message, OxiError, OxiResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::sessions_dir;

// ── Types ────────────────────────────────────────────────────────────────────

/// A single conversation branch within a `SessionTree`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBranch {
    /// Branch identifier. Also used as the resume handle for `--branch <id>`.
    pub id: Uuid,
    /// Model name used in this branch (may differ from sibling branches if user switched).
    pub model: String,
    /// `None` for the root branch; `Some(parent_branch_id)` for forked branches.
    pub parent: Option<Uuid>,
    /// At which message index of the parent we forked (`messages[0..=parent_turn]`).
    /// `None` for the root branch.
    pub parent_turn: Option<u32>,
    /// Human-readable label for the branch.
    pub title: String,
    /// Full message history for this branch, starting from the fork point.
    pub messages: Vec<Message>,
    /// When this branch was created.
    pub created_at: DateTime<Utc>,
    /// When this branch was last written.
    pub updated_at: DateTime<Utc>,
}

impl SessionBranch {
    /// Create a new root branch (no parent).
    pub fn new_root(model: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            model: model.into(),
            parent: None,
            parent_turn: None,
            title: "main".to_string(),
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Thin manifest written as `tree.json` — just the IDs needed to bootstrap load.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TreeManifest {
    session_id: Uuid,
    current: Uuid,
}

/// A session represented as a tree of branches.
///
/// All branches share `session_id`. Only one branch is "current" at a time.
#[derive(Debug, Clone)]
pub struct SessionTree {
    /// The shared session identifier (= the directory name on disk).
    pub session_id: Uuid,
    /// All branches, keyed by branch ID.
    pub branches: HashMap<Uuid, SessionBranch>,
    /// Currently active branch.
    pub current: Uuid,
}

impl SessionTree {
    // ── Constructors ──────────────────────────────────────────────────────────

    /// Build a new `SessionTree` with a single root branch.
    pub fn new(model: impl Into<String>) -> Self {
        let root = SessionBranch::new_root(model);
        let root_id = root.id;
        let session_id = Uuid::new_v4();
        let mut branches = HashMap::new();
        branches.insert(root_id, root);
        Self {
            session_id,
            branches,
            current: root_id,
        }
    }

    /// Wrap a legacy flat `Session` into a tree (migration path).
    ///
    /// The session's original UUID becomes both `session_id` and the root
    /// `branch_id` so that `--session <id>` still works without a `--branch`.
    pub fn from_legacy(session: &crate::Session) -> Self {
        let session_id: Uuid = session.id.parse().unwrap_or_else(|_| Uuid::new_v4());
        let root = SessionBranch {
            id: session_id,
            model: session.model.clone(),
            parent: None,
            parent_turn: None,
            title: session.title.clone().unwrap_or_else(|| "main".to_string()),
            messages: session.messages.clone(),
            created_at: session.created_at,
            updated_at: session.updated_at,
        };
        let mut branches = HashMap::new();
        branches.insert(session_id, root);
        Self {
            session_id,
            branches,
            current: session_id,
        }
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// Borrow the currently active branch.
    pub fn current_branch(&self) -> Option<&SessionBranch> {
        self.branches.get(&self.current)
    }

    /// Mutably borrow the currently active branch.
    pub fn current_branch_mut(&mut self) -> Option<&mut SessionBranch> {
        self.branches.get_mut(&self.current)
    }

    // ── Operations ────────────────────────────────────────────────────────────

    /// Fork the given branch at turn `at_turn`, creating a child branch.
    ///
    /// `at_turn` is a *message index* (0-based) — the new branch receives
    /// `messages[0..=at_turn]` from the parent. Switches `self.current` to
    /// the new branch and returns the new branch ID.
    ///
    /// # Errors
    /// Returns `OxiError::Session` when `parent_branch` does not exist or
    /// `at_turn` is out of range.
    pub fn fork(&mut self, parent_branch: Uuid, at_turn: u32, title: String) -> OxiResult<Uuid> {
        let messages_slice = {
            let parent = self
                .branches
                .get(&parent_branch)
                .ok_or_else(|| OxiError::Session(format!("Branch not found: {parent_branch}")))?;
            let end = at_turn as usize + 1;
            if end > parent.messages.len() {
                return Err(OxiError::Session(format!(
                    "at_turn {at_turn} is out of range (branch has {} messages)",
                    parent.messages.len()
                )));
            }
            parent.messages[..end].to_vec()
        };

        let parent_model = self
            .branches
            .get(&parent_branch)
            .map(|b| b.model.clone())
            .unwrap_or_default();

        let now = Utc::now();
        let new_id = Uuid::new_v4();
        let branch = SessionBranch {
            id: new_id,
            model: parent_model,
            parent: Some(parent_branch),
            parent_turn: Some(at_turn),
            title,
            messages: messages_slice,
            created_at: now,
            updated_at: now,
        };
        self.branches.insert(new_id, branch);
        self.current = new_id;
        Ok(new_id)
    }

    /// Switch the active branch.
    ///
    /// # Errors
    /// Returns `OxiError::Session` when `branch_id` is not in the tree.
    pub fn switch(&mut self, branch_id: Uuid) -> OxiResult<()> {
        if !self.branches.contains_key(&branch_id) {
            return Err(OxiError::Session(format!("Branch not found: {branch_id}")));
        }
        self.current = branch_id;
        Ok(())
    }

    /// Delete a branch. Refuses to delete the currently active branch.
    ///
    /// # Errors
    /// Returns `OxiError::Session` when `branch_id` is the active branch,
    /// does not exist, or is the only branch in the tree.
    pub fn delete_branch(
        &mut self,
        branch_id: Uuid,
        config_dir_override: Option<&Path>,
    ) -> OxiResult<()> {
        if branch_id == self.current {
            return Err(OxiError::Session(
                "Cannot delete the active branch. Switch first.".into(),
            ));
        }
        if self.branches.len() <= 1 {
            return Err(OxiError::Session(
                "Cannot delete the last branch in a tree.".into(),
            ));
        }
        if !self.branches.contains_key(&branch_id) {
            return Err(OxiError::Session(format!("Branch not found: {branch_id}")));
        }

        self.branches.remove(&branch_id);

        // Remove from disk if it exists.
        let branch_path =
            tree_dir(self.session_id, config_dir_override).join(format!("{branch_id}.json"));
        if branch_path.exists() {
            fs::remove_file(&branch_path)
                .map_err(|e| OxiError::Session(format!("Failed to remove branch file: {e}")))?;
        }

        Ok(())
    }

    // ── Persistence ───────────────────────────────────────────────────────────

    /// Persist every branch to disk and write the manifest.
    pub fn save(&self, config_dir_override: Option<&Path>) -> OxiResult<()> {
        let dir = tree_dir(self.session_id, config_dir_override);
        create_tree_dir(&dir)?;

        // Write each branch file.
        for branch in self.branches.values() {
            write_branch_file(&dir, branch)?;
        }

        // Write manifest.
        let manifest = TreeManifest {
            session_id: self.session_id,
            current: self.current,
        };
        write_json_file(&dir.join("tree.json"), &manifest)?;

        Ok(())
    }

    /// Load or migrate a session by its string ID.
    ///
    /// Accepts the string form of a session UUID (as stored in `Session::id`).
    /// - If a tree directory already exists, delegates to `Self::load`.
    /// - If only a legacy flat-file exists, calls `migrate_legacy_if_needed`.
    ///
    /// # Errors
    /// Returns `OxiError::Session` when the ID is invalid or neither format is found.
    pub fn load_or_migrate(
        session_id_str: &str,
        config_dir_override: Option<&Path>,
    ) -> OxiResult<Self> {
        let session_id: Uuid = session_id_str
            .parse()
            .map_err(|_| OxiError::Session(format!("Invalid session UUID: {session_id_str}")))?;

        let dir = tree_dir(session_id, config_dir_override);
        if dir.exists() {
            return Self::load(session_id, config_dir_override);
        }

        // Try legacy migration.
        migrate_legacy_if_needed(session_id_str, config_dir_override)
    }

    /// Load a `SessionTree` from disk.
    ///
    /// Reads `tree.json` manifest first, then loads each branch file referenced
    /// by the directory listing. Unknown branch files are skipped gracefully.
    ///
    /// # Errors
    /// Returns `OxiError::Session` when the manifest or any required branch
    /// file cannot be read or deserialized.
    pub fn load(session_id: Uuid, config_dir_override: Option<&Path>) -> OxiResult<Self> {
        let dir = tree_dir(session_id, config_dir_override);
        if !dir.exists() {
            return Err(OxiError::Session(format!(
                "Session tree not found: {session_id}"
            )));
        }

        let manifest_path = dir.join("tree.json");
        if !manifest_path.exists() {
            return Err(OxiError::Session(format!(
                "tree.json missing for session {session_id}"
            )));
        }
        let manifest_content = fs::read_to_string(&manifest_path)?;
        let manifest: TreeManifest = serde_json::from_str(&manifest_content)?;

        // Load all branch .json files (skip tree.json itself).
        let mut branches = HashMap::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if stem == "tree" || path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<SessionBranch>(&content) {
                    Ok(branch) => {
                        branches.insert(branch.id, branch);
                    }
                    Err(e) => {
                        tracing::warn!("Skipping malformed branch file {}: {e}", path.display());
                    }
                },
                Err(e) => {
                    tracing::warn!("Cannot read branch file {}: {e}", path.display());
                }
            }
        }

        if branches.is_empty() {
            return Err(OxiError::Session(format!(
                "No valid branches found for session {session_id}"
            )));
        }

        // Validate that `current` points to a loaded branch.
        if !branches.contains_key(&manifest.current) {
            // Fall back to any branch rather than hard-fail.
            let fallback = *branches.keys().next().ok_or_else(|| {
                OxiError::Session(
                    "Concurrent modification: branches collection became empty".into(),
                )
            })?;
            tracing::warn!(
                "Manifest.current {} not found in branches, falling back to {fallback}",
                manifest.current
            );
            return Ok(Self {
                session_id,
                branches,
                current: fallback,
            });
        }

        Ok(Self {
            session_id,
            branches,
            current: manifest.current,
        })
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Resolve the tree directory path for a session.
pub fn tree_dir(session_id: Uuid, config_dir_override: Option<&Path>) -> PathBuf {
    sessions_dir(config_dir_override).join(session_id.to_string())
}

/// Create the tree directory with mode 0o700 on Unix.
fn create_tree_dir(dir: &Path) -> OxiResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| OxiError::Session(format!("Failed to create tree dir: {e}")))?;
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
            .map_err(|e| OxiError::Session(format!("Failed to create tree dir: {e}")))?;
    }
    Ok(())
}

/// Write a `SessionBranch` to `dir/{branch_id}.json` with 0o600 perms on Unix.
fn write_branch_file(dir: &Path, branch: &SessionBranch) -> OxiResult<()> {
    let path = dir.join(format!("{}.json", branch.id));
    write_json_file(&path, branch)
}

/// Write any serializable value to a file path with 0o600 perms on Unix.
fn write_json_file<T: Serialize>(path: &Path, value: &T) -> OxiResult<()> {
    let json = serde_json::to_string_pretty(value)?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(json.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        fs::write(path, json)?;
    }
    Ok(())
}

/// Migrate a legacy flat-file session to tree format.
///
/// Reads `sessions/{id}.json`, wraps it in a `SessionTree`, writes to
/// `sessions/{id}/`, but **leaves the legacy file in place** for the
/// current release cycle (removed in a future `--migrate` pass).
///
/// Returns the loaded `SessionTree` on success.
///
/// # Errors
/// Propagates I/O and parse errors from reading the legacy file.
pub fn migrate_legacy_if_needed(
    id: &str,
    config_dir_override: Option<&Path>,
) -> OxiResult<SessionTree> {
    let sessions = sessions_dir(config_dir_override);
    let legacy_path = sessions.join(format!("{id}.json"));

    if !legacy_path.exists() {
        return Err(OxiError::Session(format!("Legacy session not found: {id}")));
    }

    let content = fs::read_to_string(&legacy_path)?;
    let session: crate::Session = serde_json::from_str(&content)?;
    let tree = SessionTree::from_legacy(&session);

    // Persist into the new tree layout (leaves legacy file intact).
    tree.save(config_dir_override)?;
    tracing::info!(
        "Migrated legacy session {} → tree format (legacy file preserved)",
        id
    );

    Ok(tree)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use oxicode_common::Message;

    fn make_tree_with_messages(n: usize) -> SessionTree {
        let mut tree = SessionTree::new("test-model");
        let branch_id = tree.current;
        let branch = tree.branches.get_mut(&branch_id).unwrap();
        for i in 0..n {
            branch.messages.push(Message::user(format!("msg {i}")));
        }
        tree
    }

    #[test]
    fn new_tree_has_one_root_branch() {
        let tree = SessionTree::new("model-x");
        assert_eq!(tree.branches.len(), 1);
        let branch = tree.current_branch().unwrap();
        assert!(branch.parent.is_none());
        assert_eq!(branch.title, "main");
    }

    #[test]
    fn fork_creates_child_branch_with_correct_messages() {
        let mut tree = make_tree_with_messages(5);
        let parent_id = tree.current;

        let child_id = tree.fork(parent_id, 2, "alt".to_string()).unwrap();
        assert_ne!(child_id, parent_id);
        assert_eq!(tree.current, child_id);

        let child = tree.branches.get(&child_id).unwrap();
        assert_eq!(child.messages.len(), 3); // [0..=2]
        assert_eq!(child.parent, Some(parent_id));
        assert_eq!(child.parent_turn, Some(2));

        // Parent still has all 5 messages.
        let parent = tree.branches.get(&parent_id).unwrap();
        assert_eq!(parent.messages.len(), 5);
    }

    #[test]
    fn fork_out_of_range_errors() {
        let mut tree = make_tree_with_messages(3);
        let parent_id = tree.current;
        let result = tree.fork(parent_id, 10, "bad".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn fork_unknown_parent_errors() {
        let mut tree = make_tree_with_messages(3);
        let unknown = Uuid::new_v4();
        let result = tree.fork(unknown, 0, "x".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn switch_updates_current() {
        let mut tree = make_tree_with_messages(4);
        let root_id = tree.current;
        let child_id = tree.fork(root_id, 1, "b".to_string()).unwrap();
        assert_eq!(tree.current, child_id);
        tree.switch(root_id).unwrap();
        assert_eq!(tree.current, root_id);
    }

    #[test]
    fn switch_unknown_branch_errors() {
        let mut tree = SessionTree::new("m");
        let bad = Uuid::new_v4();
        assert!(tree.switch(bad).is_err());
    }

    #[test]
    fn delete_non_current_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tree = make_tree_with_messages(5);
        let root_id = tree.current;
        let child_id = tree.fork(root_id, 2, "to-delete".to_string()).unwrap();
        // Switch back to root, then delete child.
        tree.switch(root_id).unwrap();
        tree.delete_branch(child_id, Some(tmp.path())).unwrap();
        assert!(!tree.branches.contains_key(&child_id));
        assert_eq!(tree.branches.len(), 1);
    }

    #[test]
    fn delete_current_branch_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tree = SessionTree::new("m");
        let current = tree.current;
        assert!(tree.delete_branch(current, Some(tmp.path())).is_err());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tree = make_tree_with_messages(4);
        let root_id = tree.current;
        let _child_id = tree.fork(root_id, 2, "b1".to_string()).unwrap();
        tree.save(Some(tmp.path())).unwrap();

        let loaded = SessionTree::load(tree.session_id, Some(tmp.path())).unwrap();
        assert_eq!(loaded.session_id, tree.session_id);
        assert_eq!(loaded.current, tree.current);
        assert_eq!(loaded.branches.len(), 2);
    }
}
