//! Tests for `SessionTree::fork`, `switch`, delete, and save/load roundtrip.

use oxicode_common::Message;
use oxicode_session::branch::{migrate_legacy_if_needed, SessionTree};
use uuid::Uuid;

fn messages(n: usize) -> Vec<Message> {
    (0..n).map(|i| Message::user(format!("turn {i}"))).collect()
}

fn tree_with_n_messages(n: usize) -> SessionTree {
    let mut tree = SessionTree::new("claude-test");
    let root_id = tree.current;
    let branch = tree.branches.get_mut(&root_id).unwrap();
    for msg in messages(n) {
        branch.messages.push(msg);
    }
    tree
}

// ── fork ─────────────────────────────────────────────────────────────────────

#[test]
fn fork_at_turn_0_gives_single_message() {
    let mut tree = tree_with_n_messages(5);
    let parent = tree.current;
    let child = tree.fork(parent, 0, "branch-a".to_string()).unwrap();
    assert_eq!(tree.branches[&child].messages.len(), 1);
    assert_eq!(tree.branches[&child].messages[0].text(), "turn 0");
}

#[test]
fn fork_at_turn_n_gives_n_plus_one_messages() {
    let mut tree = tree_with_n_messages(10);
    let parent = tree.current;
    let child = tree.fork(parent, 4, "branch-b".to_string()).unwrap();
    // messages[0..=4] → 5 messages
    assert_eq!(tree.branches[&child].messages.len(), 5);
    // Parent retains all 10.
    assert_eq!(tree.branches[&parent].messages.len(), 10);
}

#[test]
fn fork_switches_current_to_new_branch() {
    let mut tree = tree_with_n_messages(3);
    let parent = tree.current;
    let child = tree.fork(parent, 1, "alt".to_string()).unwrap();
    assert_eq!(tree.current, child);
    assert_ne!(child, parent);
}

#[test]
fn fork_sets_parent_lineage_correctly() {
    let mut tree = tree_with_n_messages(5);
    let parent_id = tree.current;
    let child_id = tree.fork(parent_id, 2, "child".to_string()).unwrap();
    let child = &tree.branches[&child_id];
    assert_eq!(child.parent, Some(parent_id));
    assert_eq!(child.parent_turn, Some(2));
}

#[test]
fn fork_at_last_index_clones_all_messages() {
    let n = 7;
    let mut tree = tree_with_n_messages(n);
    let parent_id = tree.current;
    let child_id = tree
        .fork(parent_id, (n - 1) as u32, "full-copy".to_string())
        .unwrap();
    assert_eq!(tree.branches[&child_id].messages.len(), n);
}

#[test]
fn fork_out_of_range_errors() {
    let mut tree = tree_with_n_messages(3);
    let parent_id = tree.current;
    let result = tree.fork(parent_id, 100, "bad".to_string());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("out of range") || msg.contains("messages"));
}

#[test]
fn fork_unknown_parent_errors() {
    let mut tree = tree_with_n_messages(3);
    let bad_id = Uuid::new_v4();
    let result = tree.fork(bad_id, 0, "x".to_string());
    assert!(result.is_err());
}

// ── switch ────────────────────────────────────────────────────────────────────

#[test]
fn switch_updates_current_branch() {
    let mut tree = tree_with_n_messages(5);
    let root_id = tree.current;
    let child_id = tree.fork(root_id, 2, "b".to_string()).unwrap();
    assert_eq!(tree.current, child_id);

    tree.switch(root_id).unwrap();
    assert_eq!(tree.current, root_id);
}

#[test]
fn switch_to_unknown_branch_errors() {
    let mut tree = SessionTree::new("m");
    let unknown = Uuid::new_v4();
    assert!(tree.switch(unknown).is_err());
}

#[test]
fn switch_is_idempotent_when_already_on_branch() {
    let mut tree = SessionTree::new("m");
    let current = tree.current;
    tree.switch(current).unwrap(); // no error
    assert_eq!(tree.current, current);
}

// ── delete ────────────────────────────────────────────────────────────────────

#[test]
fn delete_non_current_removes_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let mut tree = tree_with_n_messages(5);
    let root_id = tree.current;
    let child_id = tree.fork(root_id, 2, "to-remove".to_string()).unwrap();
    // Switch back so child is not current.
    tree.switch(root_id).unwrap();
    tree.delete_branch(child_id, Some(tmp.path())).unwrap();
    assert!(!tree.branches.contains_key(&child_id));
    assert_eq!(tree.branches.len(), 1);
}

#[test]
fn delete_current_branch_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let mut tree = tree_with_n_messages(3);
    let current = tree.current;
    let result = tree.delete_branch(current, Some(tmp.path()));
    assert!(result.is_err());
    // Tree is unchanged.
    assert!(tree.branches.contains_key(&current));
}

#[test]
fn delete_does_not_corrupt_remaining_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let mut tree = tree_with_n_messages(6);
    let root_id = tree.current;
    let child_a = tree.fork(root_id, 2, "a".to_string()).unwrap();
    let _child_b = tree.fork(root_id, 3, "b".to_string()).unwrap();
    tree.switch(root_id).unwrap();
    tree.delete_branch(child_a, Some(tmp.path())).unwrap();

    // Other branches intact.
    assert_eq!(tree.branches.len(), 2); // root + child_b
    assert!(tree.branches.contains_key(&root_id));
}

// ── save / load ───────────────────────────────────────────────────────────────

#[test]
fn save_then_load_preserves_all_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let mut tree = tree_with_n_messages(6);
    let root_id = tree.current;
    let child_id = tree.fork(root_id, 3, "saved-child".to_string()).unwrap();

    tree.save(Some(tmp.path())).unwrap();

    let loaded = SessionTree::load(tree.session_id, Some(tmp.path())).unwrap();
    assert_eq!(loaded.session_id, tree.session_id);
    assert_eq!(loaded.current, child_id);
    assert_eq!(loaded.branches.len(), 2);

    let loaded_child = &loaded.branches[&child_id];
    assert_eq!(loaded_child.messages.len(), 4); // [0..=3]
    assert_eq!(loaded_child.parent, Some(root_id));
    assert_eq!(loaded_child.parent_turn, Some(3));
}

#[test]
fn load_nonexistent_session_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let bad_id = Uuid::new_v4();
    let result = SessionTree::load(bad_id, Some(tmp.path()));
    assert!(result.is_err());
}

// ── migrate_legacy_if_needed ──────────────────────────────────────────────────

#[test]
fn migrate_legacy_session_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();

    // Write a legacy flat-file session.
    let mut legacy = oxicode_session::Session::new("legacy-model");
    legacy.push_message(Message::user("hello"));
    legacy.push_message(Message::user("world"));
    oxicode_session::save_session(&legacy, Some(tmp.path())).unwrap();

    // Migrate it.
    let tree = migrate_legacy_if_needed(&legacy.id, Some(tmp.path())).unwrap();

    // Tree has one branch with all messages.
    assert_eq!(tree.branches.len(), 1);
    let branch = tree.current_branch().unwrap();
    assert_eq!(branch.messages.len(), 2);
    assert_eq!(branch.messages[0].text(), "hello");
    assert_eq!(branch.messages[1].text(), "world");

    // Legacy file still exists (preserved for 1 release cycle).
    let legacy_path = tmp
        .path()
        .join(oxicode_common::constants::SESSIONS_DIR_NAME)
        .join(format!("{}.json", legacy.id));
    assert!(legacy_path.exists(), "Legacy file should be preserved");
}
