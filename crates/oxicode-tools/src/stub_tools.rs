use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Stub: Agent tool for multi-agent orchestration (Phase 4).
pub struct AgentTool;

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "agent"
    }
    fn description(&self) -> &str {
        "Launch a subagent to handle complex tasks (not yet implemented)."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string", "description": "Task for the subagent"}
                },
                "required": ["prompt"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ToolContext,
    ) -> OxiResult<ToolResult> {
        Ok(ToolResult::error(
            "Agent tool not yet implemented (Phase 4)",
        ))
    }
}

/// Stub: MCP tool for Model Context Protocol (Phase 3).
pub struct McpTool;

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        "mcp"
    }
    fn description(&self) -> &str {
        "Execute an MCP server tool (not yet implemented)."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "server": {"type": "string"},
                    "tool": {"type": "string"},
                    "input": {"type": "object"}
                },
                "required": ["server", "tool"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ToolContext,
    ) -> OxiResult<ToolResult> {
        Ok(ToolResult::error("MCP tool not yet implemented (Phase 3)"))
    }
}

/// Stub: Worktree tool for git worktree isolation (Phase 4).
pub struct WorktreeTool;

#[async_trait]
impl Tool for WorktreeTool {
    fn name(&self) -> &str {
        "worktree"
    }
    fn description(&self) -> &str {
        "Create an isolated git worktree for parallel work (not yet implemented)."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "branch": {"type": "string", "description": "Branch name for the worktree"}
                },
                "required": ["branch"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ToolContext,
    ) -> OxiResult<ToolResult> {
        Ok(ToolResult::error(
            "Worktree tool not yet implemented (Phase 4)",
        ))
    }
}
