use async_trait::async_trait;
use oxicode_agents::spawner::{AgentConfig, spawn_agent};
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Spawns a subagent process to handle complex tasks autonomously.
pub struct AgentTool;

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "agent"
    }
    fn description(&self) -> &str {
        "Launch a subagent to handle complex tasks autonomously."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "The task prompt for the subagent."
                    },
                    "model": {
                        "type": "string",
                        "description": "Model to use (default: claude-sonnet-4-20250514)."
                    },
                    "name": {
                        "type": "string",
                        "description": "Human-readable name for the subagent."
                    },
                    "description": {
                        "type": "string",
                        "description": "Brief description of the subagent's role."
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in seconds (default: 300)."
                    }
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
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> OxiResult<ToolResult> {
        let prompt = match input.get("prompt").and_then(|v| v.as_str()) {
            Some(p) => p.to_string(),
            None => return Ok(ToolResult::error("agent tool requires 'prompt' field")),
        };

        let model = input
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("claude-sonnet-4-20250514")
            .to_string();

        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("subagent")
            .to_string();

        let timeout_secs = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(300);

        let config = AgentConfig {
            name,
            prompt,
            model,
            working_dir: ctx.working_dir.clone(),
            permission_mode: "default".to_string(),
            timeout: std::time::Duration::from_secs(timeout_secs),
            inherit_env: true,
        };

        match spawn_agent(&config).await {
            Ok(result) if result.is_error => Ok(ToolResult::error(result.output)),
            Ok(result) => Ok(ToolResult::success(result.output)),
            Err(e) => Ok(ToolResult::error(format!("agent spawn failed: {e}"))),
        }
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
