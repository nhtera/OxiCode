use async_trait::async_trait;
use oxicode_agents::built_in::AgentType;
use oxicode_agents::spawner::{spawn_agent, AgentConfig};
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Spawns a subagent process to handle complex tasks autonomously.
/// Supports specialized agent types with restricted tools and tailored prompts.
pub struct AgentTool;

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "agent"
    }
    fn description(&self) -> &str {
        "Launch a subagent to handle complex tasks autonomously. Supports specialized types: plan, explore, verify, general, guide, statusline."
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
                    "subagent_type": {
                        "type": "string",
                        "description": "Specialized agent type: plan, explore, verify, general, guide, statusline. Each type has restricted tools and a tailored system prompt.",
                        "enum": ["plan", "explore", "verify", "general", "guide", "statusline"]
                    },
                    "model": {
                        "type": "string",
                        "description": "Model override (default depends on agent type)."
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
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let prompt = match input.get("prompt").and_then(|v| v.as_str()) {
            Some(p) => p.to_string(),
            None => return Ok(ToolResult::error("agent tool requires 'prompt' field")),
        };

        // Parse optional subagent_type.
        let agent_type = input
            .get("subagent_type")
            .and_then(|v| v.as_str())
            .and_then(AgentType::from_str_loose);

        let explicit_model = input.get("model").and_then(|v| v.as_str());
        let model = explicit_model
            .unwrap_or("claude-sonnet-4-20250514")
            .to_string();
        let model_override = explicit_model.is_some();

        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("subagent")
            .to_string();

        let timeout_secs = input
            .get("timeout")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(300);

        let mut config = AgentConfig {
            name,
            prompt,
            model,
            working_dir: ctx.working_dir.clone(),
            permission_mode: "default".to_string(),
            timeout: std::time::Duration::from_secs(timeout_secs),
            inherit_env: true,
            agent_type,
            allowed_tools: None,
            model_override,
        };

        // Apply agent type defaults (system prompt, model, tool whitelist).
        config.apply_agent_type();

        match spawn_agent(&config).await {
            Ok(result) if result.is_error => Ok(ToolResult::error(result.output)),
            Ok(result) => Ok(ToolResult::success(result.output)),
            Err(e) => Ok(ToolResult::error(format!("agent spawn failed: {e}"))),
        }
    }
}
