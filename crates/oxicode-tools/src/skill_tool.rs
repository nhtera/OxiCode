//! SkillTool: lets the LLM invoke discovered skills by name.
//!
//! Trust model: skills are user-installed configuration files (like CLAUDE.md),
//! discovered from `~/.oxicode/skills/` and `.oxicode/skills/`. They are treated
//! as trusted code — the LLM provides a skill name, not a file path, and paths
//! come from the discovery set only. ReadOnly permission is appropriate because
//! the tool itself only reads and returns prompt content.

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Tool that invokes a discovered skill by name, returning its prompt content.
pub struct SkillTool;

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Execute a skill within the main conversation"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "skill".into(),
            description: "Execute a skill within the main conversation. \
                When users ask you to perform tasks, check if any of the available skills match. \
                Skills provide specialized capabilities and domain knowledge. \
                When users reference a \"slash command\" or \"/<something>\", they are referring \
                to a skill. Use this tool to invoke it."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "skill": {
                        "type": "string",
                        "description": "The skill name to invoke (e.g., \"commit\", \"review-pr\")"
                    },
                    "args": {
                        "type": "string",
                        "description": "Optional arguments for the skill"
                    }
                },
                "required": ["skill"]
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let skill_name = input["skill"]
            .as_str()
            .ok_or_else(|| oxicode_common::OxiError::Other("Missing 'skill' parameter".into()))?;
        let args = input["args"].as_str().unwrap_or("");

        let Some(ref executor) = ctx.skill_executor else {
            return Ok(ToolResult::error(
                "Skill system not initialized. No skills available.",
            ));
        };

        // Find matching skill: prefer exact name match, fallback to namespace suffix.
        let skills = executor.list_skills();
        let matched = skills.iter().find(|s| s.name == skill_name).or_else(|| {
            skills.iter().find(|s| {
                s.name
                    .rsplit_once(':')
                    .is_some_and(|(_, suffix)| suffix == skill_name)
            })
        });

        let Some(info) = matched else {
            let available: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
            return Ok(ToolResult::error(format!(
                "Skill '{skill_name}' not found. Available: {}",
                if available.is_empty() {
                    "(none)".to_string()
                } else {
                    available.join(", ")
                }
            )));
        };

        // Check if skill is in the active set for this context.
        let activation_ctx = oxicode_skills::ActivationContext {
            current_file: None,
            user_input: Some(format!("/{skill_name} {args}")),
        };
        let active = executor.get_active_skills(&activation_ctx);
        let is_active = active.iter().any(|s| s.name() == info.name);

        if is_active {
            // Skill activated; return the built prompt.
            if let Some(prompt) = executor.build_skills_prompt(&activation_ctx) {
                return Ok(ToolResult::success(prompt));
            }
        }

        // Skill exists but not active for this context; read it directly.
        match tokio::fs::read_to_string(&info.source_path).await {
            Ok(content) => Ok(ToolResult::success(format!(
                "# Skill: {}\n\nARGUMENTS: {args}\n\n{content}",
                info.name
            ))),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to read skill '{}': {e}",
                info.name
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_tool_schema_valid() {
        let tool = SkillTool;
        let schema = tool.schema();
        assert_eq!(schema.name, "skill");
        let required = schema.input_schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "skill");
    }

    #[test]
    fn skill_tool_name_and_permission() {
        let tool = SkillTool;
        assert_eq!(tool.name(), "skill");
        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
    }
}
