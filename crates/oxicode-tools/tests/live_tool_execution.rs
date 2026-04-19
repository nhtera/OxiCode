//! Integration tests for core tool execution against real filesystem.
//!
//! These tests do NOT require API credentials — they test tool logic directly.
//! Run with: `cargo test -p oxicode-tools --test live_tool_execution`

use std::path::Path;
use std::sync::{Arc, Mutex};

use oxicode_tools::file_state_tracker::FileStateTracker;
use oxicode_tools::tool_trait::ToolContext;
use oxicode_tools::ToolRegistry;

/// Build a minimal ToolContext pointing at the given temp directory.
fn make_tool_context(dir: &Path) -> ToolContext {
    ToolContext {
        working_dir: dir.to_path_buf(),
        file_state: Arc::new(FileStateTracker::default()),
        task_manager: Arc::new(Mutex::new(oxicode_tasks::TaskManager::default())),
        task_abort_handles: Arc::new(Mutex::new(std::collections::HashMap::new())),
        mcp_manager: Arc::new(oxicode_mcp::McpServerManager::new()),
        skill_executor: None,
        team_manager: Arc::new(Mutex::new(oxicode_agents::TeamManager::new())),
        bash_processes: Arc::new(Mutex::new(std::collections::HashMap::new())),
        hook_manager: None,
        permission_mode: oxicode_permissions::PermissionMode::Default,
    }
}

/// Build a registry with only core tools for focused testing.
fn core_registry() -> ToolRegistry {
    oxicode_tools::default_registry()
}

// ── file_read ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_file_read_tool() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("hello.txt");
    std::fs::write(&file_path, "Hello from test!").unwrap();

    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap()
    });

    let result = reg.execute("file_read", input, &ctx).await.unwrap();
    assert!(
        !result.is_error,
        "file_read should succeed: {}",
        result.content
    );
    assert!(
        result.content.contains("Hello from test!"),
        "Should contain file content, got: {}",
        result.content
    );
}

#[tokio::test]
async fn test_file_read_nonexistent() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    let input = serde_json::json!({
        "file_path": tmp.path().join("does_not_exist.txt").to_str().unwrap()
    });

    let result = reg.execute("file_read", input, &ctx).await.unwrap();
    assert!(
        result.is_error,
        "file_read of nonexistent file should return error"
    );
}

// ── file_write ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_file_write_tool() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("output.txt");

    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "Written by test"
    });

    let result = reg.execute("file_write", input, &ctx).await.unwrap();
    assert!(
        !result.is_error,
        "file_write should succeed: {}",
        result.content
    );

    // Verify file exists on disk.
    let on_disk = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(on_disk, "Written by test");
}

// ── file_edit ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_file_edit_tool() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("edit_me.txt");
    std::fs::write(&file_path, "Hello OLD world").unwrap();

    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    // First, "read" the file so the file state tracker records it (required by Edit tool).
    let read_input = serde_json::json!({"file_path": file_path.to_str().unwrap()});
    let _ = reg.execute("file_read", read_input, &ctx).await;

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "OLD",
        "new_string": "NEW"
    });

    let result = reg.execute("file_edit", input, &ctx).await.unwrap();
    assert!(
        !result.is_error,
        "file_edit should succeed: {}",
        result.content
    );

    let on_disk = std::fs::read_to_string(&file_path).unwrap();
    assert!(
        on_disk.contains("NEW"),
        "File should contain 'NEW' after edit, got: {on_disk}"
    );
    assert!(
        !on_disk.contains("OLD"),
        "File should not contain 'OLD' after edit, got: {on_disk}"
    );
}

// ── glob ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_glob_tool() {
    let tmp = tempfile::tempdir().unwrap();
    // Create a few files.
    std::fs::write(tmp.path().join("foo.rs"), "").unwrap();
    std::fs::write(tmp.path().join("bar.rs"), "").unwrap();
    std::fs::write(tmp.path().join("baz.txt"), "").unwrap();

    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    let input = serde_json::json!({
        "pattern": "*.rs",
        "path": tmp.path().to_str().unwrap()
    });

    let result = reg.execute("glob", input, &ctx).await.unwrap();
    assert!(!result.is_error, "glob should succeed: {}", result.content);
    assert!(
        result.content.contains("foo.rs"),
        "Should find foo.rs, got: {}",
        result.content
    );
    assert!(
        result.content.contains("bar.rs"),
        "Should find bar.rs, got: {}",
        result.content
    );
    // baz.txt should NOT match *.rs.
    assert!(
        !result.content.contains("baz.txt"),
        "baz.txt should not match *.rs"
    );
}

// ── grep ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_grep_tool() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("source.rs"),
        "fn main() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("other.rs"), "fn other() {}\n").unwrap();

    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    let input = serde_json::json!({
        "pattern": "println",
        "path": tmp.path().to_str().unwrap()
    });

    let result = reg.execute("grep", input, &ctx).await.unwrap();
    assert!(!result.is_error, "grep should succeed: {}", result.content);
    assert!(
        result.content.contains("source.rs"),
        "Should match source.rs, got: {}",
        result.content
    );
}

// ── bash ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_bash_tool_echo() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    let input = serde_json::json!({
        "command": "echo hello"
    });

    let result = reg.execute("bash", input, &ctx).await.unwrap();
    assert!(
        !result.is_error,
        "bash echo should succeed: {}",
        result.content
    );
    assert!(
        result.content.contains("hello"),
        "Output should contain 'hello', got: {}",
        result.content
    );
}

#[tokio::test]
async fn test_bash_tool_error_exit() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    let input = serde_json::json!({
        "command": "false"
    });

    let result = reg.execute("bash", input, &ctx).await.unwrap();
    assert!(result.is_error, "bash `false` should return is_error=true");
}

#[tokio::test]
async fn test_bash_tool_timeout() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    let input = serde_json::json!({
        "command": "sleep 999",
        "timeout": 1000
    });

    let result = reg.execute("bash", input, &ctx).await.unwrap();
    assert!(
        result.is_error,
        "bash with 1s timeout on `sleep 999` should error: {}",
        result.content
    );
    // Should mention timeout in the output.
    let lower = result.content.to_lowercase();
    assert!(
        lower.contains("timeout") || lower.contains("timed out") || lower.contains("killed"),
        "Error should mention timeout, got: {}",
        result.content
    );
}

#[tokio::test]
async fn test_bash_tool_working_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    let input = serde_json::json!({
        "command": "pwd"
    });

    let result = reg.execute("bash", input, &ctx).await.unwrap();
    assert!(!result.is_error, "bash pwd should succeed");
    // The output should contain the temp dir path.
    let tmp_canonical = tmp.path().canonicalize().unwrap();
    let content_trimmed = result.content.trim();
    // On macOS /tmp is /private/tmp, so canonicalize for comparison.
    let output_canonical = std::path::Path::new(content_trimmed)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(content_trimmed));
    assert_eq!(
        output_canonical, tmp_canonical,
        "Working dir should be temp dir"
    );
}

// ── notebook_edit ──────────────────────────────────────────────────

#[tokio::test]
async fn test_notebook_edit_tool_exists() {
    // Just verify the tool is registered (notebook files are complex to test).
    let reg = core_registry();
    assert!(
        reg.get("notebook_edit").is_some(),
        "notebook_edit tool should be registered"
    );
}

// ── web_fetch ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_web_fetch_tool_exists() {
    let reg = core_registry();
    assert!(
        reg.get("web_fetch").is_some(),
        "web_fetch tool should be registered"
    );
}

// ── web_search ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_web_search_tool_exists() {
    let reg = core_registry();
    assert!(
        reg.get("web_search").is_some(),
        "web_search tool should be registered"
    );
}

// ── task tools ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_task_create_and_list() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    // Create a task (TaskCreate in this crate expects type=bash|agent, not subject/description).
    let create_input = serde_json::json!({
        "type": "bash",
        "command": "echo task_test"
    });
    let create_result = reg
        .execute("task_create", create_input, &ctx)
        .await
        .unwrap();
    assert!(
        !create_result.is_error,
        "task_create should succeed: {}",
        create_result.content
    );

    // List tasks.
    let list_input = serde_json::json!({});
    let list_result = reg.execute("task_list", list_input, &ctx).await.unwrap();
    assert!(
        !list_result.is_error,
        "task_list should succeed: {}",
        list_result.content
    );
    assert!(
        list_result.content.contains("echo task_test") || list_result.content.contains("LocalBash"),
        "Task list should contain our task, got: {}",
        list_result.content
    );
}

// ── All core tools are registered ──────────────────────────────────

#[test]
fn test_all_core_tools_registered() {
    let reg = core_registry();
    let expected_tools = [
        "file_read",
        "file_write",
        "file_edit",
        "glob",
        "grep",
        "bash",
        "notebook_edit",
        "ask_user",
        "send_message",
        "config",
        "tool_search",
        "Task",
        "web_fetch",
        "web_search",
        "task_create",
        "task_get",
        "task_list",
        "task_update",
        "task_stop",
        "task_output",
        "skill",
    ];
    for name in &expected_tools {
        assert!(
            reg.get(name).is_some(),
            "Tool '{name}' should be registered"
        );
    }
}
