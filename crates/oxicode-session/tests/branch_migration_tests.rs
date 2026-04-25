//! Tests for legacy flat-file → tree format migration.

use oxicode_common::Message;
use oxicode_session::branch::{migrate_legacy_if_needed, SessionTree};
use oxicode_session::{list_sessions, load_session, save_session, Session};

// Returns (TempDir guard, config_dir path). The TempDir must be kept alive.
fn tmp_sessions_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().to_path_buf();
    (tmp, config_dir)
}

// ── migrate_legacy_if_needed ──────────────────────────────────────────────────

#[test]
fn migration_preserves_message_count_and_content() {
    let (tmp, config_dir) = tmp_sessions_dir();

    let mut session = Session::new("claude-sonnet-4-20250514");
    session.push_message(Message::user("first message"));
    session.push_message(Message::user("second message"));
    session.push_message(Message::user("third message"));
    save_session(&session, Some(&config_dir)).unwrap();

    let tree = migrate_legacy_if_needed(&session.id, Some(&config_dir)).unwrap();

    let branch = tree.current_branch().unwrap();
    assert_eq!(branch.messages.len(), 3);
    assert_eq!(branch.messages[0].text(), "first message");
    assert_eq!(branch.messages[1].text(), "second message");
    assert_eq!(branch.messages[2].text(), "third message");

    drop(tmp);
}

#[test]
fn migration_preserves_model_name() {
    let (tmp, config_dir) = tmp_sessions_dir();
    let mut session = Session::new("claude-opus-4-20250514");
    session.push_message(Message::user("hi"));
    save_session(&session, Some(&config_dir)).unwrap();

    let tree = migrate_legacy_if_needed(&session.id, Some(&config_dir)).unwrap();
    assert_eq!(
        tree.current_branch().unwrap().model,
        "claude-opus-4-20250514"
    );

    drop(tmp);
}

#[test]
fn migration_preserves_timestamps() {
    let (tmp, config_dir) = tmp_sessions_dir();
    let mut session = Session::new("m");
    session.push_message(Message::user("ts-test"));
    save_session(&session, Some(&config_dir)).unwrap();

    let tree = migrate_legacy_if_needed(&session.id, Some(&config_dir)).unwrap();
    let branch = tree.current_branch().unwrap();

    assert_eq!(branch.created_at, session.created_at);
    assert_eq!(branch.updated_at, session.updated_at);

    drop(tmp);
}

#[test]
fn migration_leaves_legacy_file_intact() {
    let (tmp, config_dir) = tmp_sessions_dir();
    let mut session = Session::new("m");
    session.push_message(Message::user("preserve-me"));
    save_session(&session, Some(&config_dir)).unwrap();

    migrate_legacy_if_needed(&session.id, Some(&config_dir)).unwrap();

    // Legacy file must still exist.
    let legacy_path = config_dir
        .join(oxicode_common::constants::SESSIONS_DIR_NAME)
        .join(format!("{}.json", session.id));
    assert!(
        legacy_path.exists(),
        "Legacy flat-file should be preserved after migration"
    );

    drop(tmp);
}

#[test]
fn migration_writes_tree_dir_with_manifest_and_branch() {
    let (tmp, config_dir) = tmp_sessions_dir();
    let mut session = Session::new("m");
    session.push_message(Message::user("check-tree-dir"));
    save_session(&session, Some(&config_dir)).unwrap();

    let tree = migrate_legacy_if_needed(&session.id, Some(&config_dir)).unwrap();
    let sessions_root = config_dir.join(oxicode_common::constants::SESSIONS_DIR_NAME);

    // Tree directory exists.
    let tree_dir = sessions_root.join(&session.id);
    assert!(tree_dir.exists(), "Tree dir should be created");

    // Manifest exists.
    let manifest = tree_dir.join("tree.json");
    assert!(manifest.exists(), "tree.json manifest should be written");

    // Branch file exists.
    let branch_file = tree_dir.join(format!("{}.json", tree.current));
    assert!(branch_file.exists(), "Branch file should exist");

    drop(tmp);
}

#[test]
fn migration_fails_gracefully_for_nonexistent_session() {
    let (tmp, config_dir) = tmp_sessions_dir();
    let result = migrate_legacy_if_needed("nonexistent-id", Some(&config_dir));
    assert!(result.is_err());
    drop(tmp);
}

// ── load_session compat ───────────────────────────────────────────────────────

#[test]
fn load_session_falls_back_to_legacy_flat_file() {
    let (tmp, config_dir) = tmp_sessions_dir();
    let mut session = Session::new("legacy-model");
    session.push_message(Message::user("legacy content"));
    save_session(&session, Some(&config_dir)).unwrap();

    // Load via the standard API — should find the legacy file.
    let loaded = load_session(&session.id, Some(&config_dir)).unwrap();
    assert_eq!(loaded.messages.len(), 1);
    assert_eq!(loaded.messages[0].text(), "legacy content");

    drop(tmp);
}

#[test]
fn load_session_prefers_tree_root_over_legacy_after_explicit_migration() {
    let (tmp, config_dir) = tmp_sessions_dir();

    // Create legacy session.
    let mut session = Session::new("m");
    session.push_message(Message::user("legacy msg"));
    save_session(&session, Some(&config_dir)).unwrap();

    // Explicitly migrate so both legacy + tree exist.
    migrate_legacy_if_needed(&session.id, Some(&config_dir)).unwrap();

    // load_session resolves the tree root branch (sessions/{id}/{id}.json).
    let loaded = load_session(&session.id, Some(&config_dir)).unwrap();
    assert_eq!(loaded.messages.len(), 1);
    assert_eq!(loaded.messages[0].text(), "legacy msg");

    drop(tmp);
}

// ── list_sessions discovers tree sessions ─────────────────────────────────────

#[test]
fn list_sessions_returns_tree_sessions() {
    let (tmp, config_dir) = tmp_sessions_dir();

    // Create a tree-format session directly.
    let mut tree = SessionTree::new("tree-model");
    let root_id = tree.current;
    tree.branches
        .get_mut(&root_id)
        .unwrap()
        .messages
        .push(Message::user("tree message"));
    tree.save(Some(&config_dir)).unwrap();

    let summaries = list_sessions(Some(&config_dir)).unwrap();
    assert_eq!(summaries.len(), 1, "Should find the tree session");
    assert_eq!(summaries[0].model, "tree-model");
    assert_eq!(summaries[0].message_count, 1);

    drop(tmp);
}

#[test]
fn list_sessions_does_not_double_count_migrated_sessions() {
    let (tmp, config_dir) = tmp_sessions_dir();

    // Write a legacy session and migrate it (both flat file + tree dir exist).
    let mut session = Session::new("m");
    session.push_message(Message::user("migrated"));
    save_session(&session, Some(&config_dir)).unwrap();
    migrate_legacy_if_needed(&session.id, Some(&config_dir)).unwrap();

    // Should appear exactly once even though both legacy + tree exist.
    let summaries = list_sessions(Some(&config_dir)).unwrap();
    assert_eq!(summaries.len(), 1, "Migrated session should appear once");

    drop(tmp);
}

#[test]
fn list_sessions_mixed_legacy_and_tree() {
    let (tmp, config_dir) = tmp_sessions_dir();

    // Legacy-only session (no migration).
    let mut s1 = Session::new("model-a");
    s1.push_message(Message::user("legacy-only"));
    save_session(&s1, Some(&config_dir)).unwrap();

    // Tree-only session.
    let mut tree = SessionTree::new("model-b");
    let root = tree.current;
    tree.branches
        .get_mut(&root)
        .unwrap()
        .messages
        .push(Message::user("tree-only"));
    tree.save(Some(&config_dir)).unwrap();

    let summaries = list_sessions(Some(&config_dir)).unwrap();
    assert_eq!(summaries.len(), 2);

    let models: std::collections::HashSet<&str> =
        summaries.iter().map(|s| s.model.as_str()).collect();
    assert!(models.contains("model-a"));
    assert!(models.contains("model-b"));

    drop(tmp);
}
