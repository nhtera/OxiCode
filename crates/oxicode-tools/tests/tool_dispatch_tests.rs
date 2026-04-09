//! Tests for tool dispatch: sequential execution, error isolation, unknown tool handling.
//!
//! No API key needed — uses ToolRegistry directly.
//! Run with: `cargo test -p oxicode-tools --test tool_dispatch_tests`

use std::path::Path;
use std::sync::{Arc, Mutex};

use oxicode_tools::file_state_tracker::FileStateTracker;
use oxicode_tools::tool_trait::ToolContext;
use oxicode_tools::ToolRegistry;

/// Build a minimal ToolContext pointing at the given directory.
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
    }
}

fn core_registry() -> ToolRegistry {
    oxicode_tools::default_registry()
}

#[tokio::test]
async fn test_multiple_tools_execute_in_sequence() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    let file_path = tmp.path().join("seq_test.txt");

    // Step 1: Write a file.
    let write_input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "sequential write test"
    });
    let write_result = reg.execute("file_write", write_input, &ctx).await.unwrap();
    assert!(
        !write_result.is_error,
        "file_write should succeed: {}",
        write_result.content
    );

    // Step 2: Read the same file — should see the written content (ordering guarantee).
    let read_input = serde_json::json!({
        "file_path": file_path.to_str().unwrap()
    });
    let read_result = reg.execute("file_read", read_input, &ctx).await.unwrap();
    assert!(
        !read_result.is_error,
        "file_read should succeed: {}",
        read_result.content
    );
    assert!(
        read_result.content.contains("sequential write test"),
        "read should see write's content, got: {}",
        read_result.content
    );
}

#[tokio::test]
async fn test_tool_error_does_not_stop_batch() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    // First tool: read a nonexistent file → should return an error result.
    let bad_input = serde_json::json!({
        "file_path": tmp.path().join("nonexistent_file_abc.txt").to_str().unwrap()
    });
    let result_1 = reg.execute("file_read", bad_input, &ctx).await.unwrap();
    assert!(result_1.is_error, "reading nonexistent file should error");

    // Second tool: write a valid file — should still execute fine.
    let good_path = tmp.path().join("valid.txt");
    let good_input = serde_json::json!({
        "file_path": good_path.to_str().unwrap(),
        "content": "still works"
    });
    let result_2 = reg.execute("file_write", good_input, &ctx).await.unwrap();
    assert!(
        !result_2.is_error,
        "second tool should execute despite first error: {}",
        result_2.content
    );
}

#[tokio::test]
async fn test_unknown_tool_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    let input = serde_json::json!({"some": "data"});
    let result = reg.execute("nonexistent_tool_xyz", input, &ctx).await;

    assert!(result.is_err(), "unknown tool should return Err");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("not found") || err_msg.contains("nonexistent_tool_xyz"),
        "error should mention the tool name, got: {err_msg}"
    );
}

#[tokio::test]
async fn test_tool_result_contains_correct_content() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = core_registry();
    let ctx = make_tool_context(tmp.path());

    // Use structured_output tool — simple, no side effects.
    let input = serde_json::json!({
        "data": {"key": "value"}
    });
    let result = reg.execute("structured_output", input, &ctx).await.unwrap();
    assert!(!result.is_error);

    // Verify the result content is valid JSON containing our data.
    let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(parsed["key"], "value");
}
