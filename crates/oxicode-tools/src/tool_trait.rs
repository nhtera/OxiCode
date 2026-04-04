use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use oxicode_agents::TeamManager;
use oxicode_common::OxiResult;
use oxicode_mcp::McpServerManager;
use oxicode_skills::SkillExecutor;
use oxicode_tasks::TaskManager;
use serde::{Deserialize, Serialize};

use crate::file_state_tracker::FileStateTracker;

/// Metadata for a running bash process, used by KillBashTool.
#[derive(Debug, Clone)]
pub struct BashProcess {
    pub pid: u32,
    pub command: String,
    pub started_at: std::time::Instant,
}

/// Shared map of task_id → running bash process info.
/// Inserted on bash spawn, removed on completion/timeout.
pub type BashProcessMap = Arc<Mutex<HashMap<String, BashProcess>>>;

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Output content (text, JSON, etc.).
    pub content: String,
    /// Whether the tool encountered an error.
    pub is_error: bool,
}

impl ToolResult {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

/// Context provided to tools during execution.
#[derive(Clone)]
pub struct ToolContext {
    /// Working directory for file operations and command execution.
    pub working_dir: PathBuf,
    /// Tracks file modification times to detect stale edits.
    pub file_state: Arc<FileStateTracker>,
    /// Background task manager.
    pub task_manager: Arc<Mutex<TaskManager>>,
    /// Abort handles for running background tasks (for TaskStop).
    pub task_abort_handles: Arc<Mutex<HashMap<String, tokio::task::AbortHandle>>>,
    /// MCP server manager for executing MCP tools.
    pub mcp_manager: Arc<McpServerManager>,
    /// Skill executor for invoking discovered skills (None if not initialized).
    pub skill_executor: Option<Arc<SkillExecutor>>,
    /// Team manager for creating/deleting agent teams.
    pub team_manager: Arc<Mutex<TeamManager>>,
    /// Tracks running bash processes by task_id for KillBashTool.
    pub bash_processes: BashProcessMap,
}

impl fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolContext")
            .field("working_dir", &self.working_dir)
            .finish_non_exhaustive()
    }
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            file_state: Arc::new(FileStateTracker::default()),
            task_manager: Arc::new(Mutex::new(TaskManager::default())),
            task_abort_handles: Arc::new(Mutex::new(HashMap::new())),
            mcp_manager: Arc::new(McpServerManager::default()),
            skill_executor: None,
            team_manager: Arc::new(Mutex::new(TeamManager::new())),
            bash_processes: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// JSON Schema description of a tool for the LLM API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Permission level required by a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionLevel {
    /// Safe read-only operations (auto-allowed).
    ReadOnly,
    /// File write/edit operations.
    FileWrite,
    /// Shell command execution.
    ShellExec,
    /// System-level operations.
    System,
}

/// Core trait that all tools must implement.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool name (e.g., "bash", "file_read").
    fn name(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str;

    /// JSON Schema for this tool's input parameters.
    fn schema(&self) -> ToolSchema;

    /// Permission level required to execute this tool.
    fn permission_level(&self) -> PermissionLevel;

    /// Execute the tool with the given JSON input and context.
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_result_success() {
        let result = ToolResult::success("output");
        assert!(!result.is_error);
        assert_eq!(result.content, "output");
    }

    #[test]
    fn tool_result_error() {
        let result = ToolResult::error("something went wrong");
        assert!(result.is_error);
        assert_eq!(result.content, "something went wrong");
    }

    #[test]
    fn tool_result_serde_roundtrip() {
        let result = ToolResult::success("data");
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "data");
        assert!(!parsed.is_error);
    }

    #[test]
    fn permission_level_serde() {
        let levels = [
            PermissionLevel::ReadOnly,
            PermissionLevel::FileWrite,
            PermissionLevel::ShellExec,
            PermissionLevel::System,
        ];
        for level in levels {
            let json = serde_json::to_string(&level).unwrap();
            let parsed: PermissionLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, level);
        }
    }

    #[test]
    fn tool_context_default() {
        let ctx = ToolContext::default();
        assert!(!ctx.working_dir.as_os_str().is_empty());
    }

    #[test]
    fn tool_context_debug() {
        let ctx = ToolContext::default();
        let debug = format!("{ctx:?}");
        assert!(debug.contains("ToolContext"));
        assert!(debug.contains("working_dir"));
    }

    #[test]
    fn tool_schema_construction() {
        let schema = ToolSchema {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        assert_eq!(schema.name, "test_tool");
        assert_eq!(schema.description, "A test tool");
    }

    #[test]
    fn tool_result_empty_content() {
        let result = ToolResult::success("");
        assert!(!result.is_error);
        assert!(result.content.is_empty());
    }
}
