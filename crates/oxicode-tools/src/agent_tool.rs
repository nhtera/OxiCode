use async_trait::async_trait;
use oxicode_agents::built_in::AgentType;
use oxicode_agents::loader as agent_loader;
use oxicode_agents::spawner::{spawn_agent, AgentConfig};
use oxicode_common::OxiResult;
use oxicode_permissions::PermissionMode;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Spawns a subagent process to handle complex tasks autonomously.
/// Supports specialized agent types with restricted tools and tailored prompts.
pub struct AgentTool;

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "Task"
    }
    fn description(&self) -> &str {
        "Launch a subagent to handle complex tasks autonomously. Supports specialized types: plan, explore, verify, general, guide, statusline."
    }
    fn schema(&self) -> ToolSchema {
        // Built-in slugs + any user-defined agents discovered from
        // ~/.claude/agents/*.md and project .claude/agents/*.md.
        let mut subagent_types: Vec<String> = vec![
            "plan".into(),
            "explore".into(),
            "verify".into(),
            "general".into(),
            "guide".into(),
            "statusline".into(),
        ];
        for agent in agent_loader::discover() {
            if !subagent_types.iter().any(|n| n == &agent.name) {
                subagent_types.push(agent.name.clone());
            }
        }

        let custom_blurb = agent_loader::discover()
            .iter()
            .map(|a| format!("- {}: {}", a.name, a.description))
            .collect::<Vec<_>>()
            .join("\n");
        let description = if custom_blurb.is_empty() {
            "Specialized agent type: plan, explore, verify, general, guide, statusline. Each type has restricted tools and a tailored system prompt.".to_string()
        } else {
            format!(
                "Specialized agent type: plan, explore, verify, general, guide, statusline, or one of the user-defined agents below.\n\nUser-defined agents:\n{custom_blurb}"
            )
        };

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
                        "description": description,
                        "enum": subagent_types
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
                    "permission_mode": {
                        "type": "string",
                        "description": "Permission mode for the subagent: bypass, approval_only, default.",
                        "enum": ["bypass", "approval_only", "default"]
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

        // Parse optional subagent_type. First try to match a built-in enum
        // variant; if that fails, look for a user-defined agent loaded from
        // ~/.claude/agents/*.md or project .claude/agents/*.md.
        let subagent_type_str = input
            .get("subagent_type")
            .and_then(|v| v.as_str())
            .map(String::from);
        let agent_type = subagent_type_str
            .as_deref()
            .and_then(AgentType::from_str_loose);
        let custom_agent = if agent_type.is_none() {
            subagent_type_str
                .as_deref()
                .and_then(agent_loader::find_by_name)
        } else {
            None
        };

        let explicit_model = input.get("model").and_then(|v| v.as_str());
        // Model precedence: explicit `model` arg > custom agent's frontmatter
        // `model` > ANTHROPIC_DEFAULT_SONNET_MODEL env > current stable Sonnet.
        let env_default = std::env::var("ANTHROPIC_DEFAULT_SONNET_MODEL").ok();
        let custom_model = custom_agent.as_ref().and_then(|a| a.model.clone());
        let model = explicit_model
            .map(str::to_string)
            .or(custom_model)
            .or(env_default)
            .unwrap_or_else(|| "claude-sonnet-4-5-20250929".to_string());
        let model_override = explicit_model.is_some() || custom_agent.is_some();

        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("subagent")
            .to_string();

        let timeout_secs = input
            .get("timeout")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(300);

        // Permission mode inheritance: parent bypass/approval_only wins;
        // otherwise child's declared mode (or default).
        let declared = input
            .get("permission_mode")
            .and_then(|v| v.as_str())
            .map(PermissionMode::parse);
        let effective = match ctx.permission_mode {
            PermissionMode::Bypass | PermissionMode::ApprovalOnly => ctx.permission_mode,
            PermissionMode::Default => declared.unwrap_or(PermissionMode::Default),
        };
        let permission_mode_str = match effective {
            PermissionMode::Bypass => "bypass",
            PermissionMode::ApprovalOnly => "approval_only",
            PermissionMode::Default => "default",
        }
        .to_string();

        // Compose the final prompt. For user-defined agents, prepend the
        // frontmatter system prompt to the user's task prompt (matches the
        // built-in AgentType behavior).
        let final_prompt = if let Some(ref ca) = custom_agent {
            format!("{}\n\n---\n\n{}", ca.system_prompt, prompt)
        } else {
            prompt
        };
        let allowed_tools_override = custom_agent.as_ref().and_then(|a| a.tools.clone());

        let mut config = AgentConfig {
            name,
            prompt: final_prompt,
            model,
            working_dir: ctx.working_dir.clone(),
            permission_mode: permission_mode_str,
            timeout: std::time::Duration::from_secs(timeout_secs),
            inherit_env: true,
            agent_type,
            allowed_tools: allowed_tools_override,
            model_override,
        };

        // Apply agent type defaults (only has effect when a built-in variant
        // was chosen — custom agents already supplied prompt/tools above).
        config.apply_agent_type();

        // Pass hook_manager to spawn_agent for AgentSpawn/AgentComplete events.
        let hm = ctx.hook_manager.as_ref();
        match spawn_agent(&config, hm).await {
            Ok(result) if result.is_error => Ok(ToolResult::error(result.output)),
            Ok(result) => {
                let output = unwrap_agent_output(&result.output);
                Ok(ToolResult::success(output))
            }
            Err(e) => Ok(ToolResult::error(format!("agent spawn failed: {e}"))),
        }
    }
}

/// Extract the `output` field from a child's JSON envelope, or return raw text.
/// Child emits: `{"agent_id":"...","output":"...","is_error":false,"duration_ms":N}`
fn unwrap_agent_output(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(output) = obj.get("output").and_then(|v| v.as_str()) {
            if !output.is_empty() {
                return output.to_string();
            }
        }
        if let Some(err) = obj.get("error").and_then(|v| v.as_str()) {
            if !err.is_empty() {
                return err.to_string();
            }
        }
    }
    trimmed.to_string()
}
