//! Tests for StructuredOutputTool — JSON extraction, validation, error handling.
//!
//! No API key needed — pure tool logic.
//! Run with: `cargo test -p oxicode-tools --test structured_output_tests`

use std::path::Path;
use std::sync::{Arc, Mutex};

use oxicode_tools::file_state_tracker::FileStateTracker;
use oxicode_tools::structured_output::StructuredOutputTool;
use oxicode_tools::tool_trait::ToolContext;
use oxicode_tools::{PermissionLevel, Tool};

/// Build a minimal ToolContext for testing.
fn make_tool_context(dir: &Path) -> ToolContext {
    ToolContext {
        working_dir: dir.to_path_buf(),
        extra_working_dirs: Vec::new(),
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

#[tokio::test]
async fn test_structured_output_returns_pretty_json() {
    let tool = StructuredOutputTool;
    let ctx = make_tool_context(Path::new("/tmp"));

    let input = serde_json::json!({
        "data": {"name": "test", "value": 42}
    });

    let result = tool.execute(input, &ctx).await.unwrap();
    assert!(!result.is_error);

    // Verify output is pretty-printed (contains newlines and indentation).
    assert!(result.content.contains('\n'), "should be pretty-printed");
    let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(parsed["name"], "test");
    assert_eq!(parsed["value"], 42);
}

#[tokio::test]
async fn test_structured_output_missing_data_field_errors() {
    let tool = StructuredOutputTool;
    let ctx = make_tool_context(Path::new("/tmp"));

    let input = serde_json::json!({
        "schema_name": "foo"
    });

    let result = tool.execute(input, &ctx).await.unwrap();
    assert!(result.is_error, "should error when data field is missing");
    assert!(
        result.content.contains("data field is required"),
        "error message should mention 'data field is required', got: {}",
        result.content
    );
}

#[tokio::test]
async fn test_structured_output_nested_objects_preserved() {
    let tool = StructuredOutputTool;
    let ctx = make_tool_context(Path::new("/tmp"));

    let input = serde_json::json!({
        "data": {
            "level1": {
                "level2": {
                    "level3": {
                        "deep_value": "hello"
                    }
                }
            }
        }
    });

    let result = tool.execute(input, &ctx).await.unwrap();
    assert!(!result.is_error);

    let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(parsed["level1"]["level2"]["level3"]["deep_value"], "hello");
}

#[tokio::test]
async fn test_structured_output_array_data() {
    let tool = StructuredOutputTool;
    let ctx = make_tool_context(Path::new("/tmp"));

    let input = serde_json::json!({
        "data": [1, 2, 3]
    });

    let result = tool.execute(input, &ctx).await.unwrap();
    assert!(!result.is_error);

    let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn test_structured_output_tool_is_readonly() {
    let tool = StructuredOutputTool;
    assert_eq!(
        tool.permission_level(),
        PermissionLevel::ReadOnly,
        "StructuredOutputTool should be ReadOnly"
    );
}
