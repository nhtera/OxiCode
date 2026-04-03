use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use oxicode_common::OxiResult;
use oxicode_agents::TeamManager;
use oxicode_mcp::McpServerManager;
use oxicode_skills::SkillExecutor;
use oxicode_tasks::TaskManager;
use serde::{Deserialize, Serialize};

use crate::file_state_tracker::FileStateTracker;

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
