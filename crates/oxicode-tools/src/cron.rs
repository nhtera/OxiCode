//! Cron tools: create, delete, list scheduled tasks.
//!
//! Schedules are stored as JSON files in ~/.oxicode/schedules/.

use std::path::PathBuf;

use async_trait::async_trait;
use oxicode_common::OxiResult;
use serde::{Deserialize, Serialize};

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// A persisted schedule entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScheduleEntry {
    id: String,
    cron: String,
    command: String,
    #[serde(default)]
    description: String,
    created_at: String,
}

fn schedules_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oxicode")
        .join("schedules")
}

/// Create a new cron schedule.
pub struct CronCreateTool;

#[async_trait]
impl Tool for CronCreateTool {
    fn name(&self) -> &str {
        "cron_create"
    }
    fn description(&self) -> &str {
        "Create a scheduled recurring task with a cron expression."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "cron": { "type": "string", "description": "Cron expression (e.g. '0 * * * *')" },
                    "command": { "type": "string", "description": "Command or prompt to execute" },
                    "description": { "type": "string", "description": "Human-readable description" }
                },
                "required": ["cron", "command"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(cron) = input.get("cron").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("cron expression required"));
        };
        let Some(command) = input.get("command").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("command required"));
        };
        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let id = uuid::Uuid::new_v4().to_string();
        let entry = ScheduleEntry {
            id: id.clone(),
            cron: cron.to_string(),
            command: command.to_string(),
            description: description.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let dir = schedules_dir();
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("{id}.json"));
        let json = serde_json::to_string_pretty(&entry)
            .map_err(|e| oxicode_common::OxiError::Other(format!("Serialize failed: {e}")))?;
        std::fs::write(&path, json)?;

        Ok(ToolResult::success(format!(
            "Schedule created: {id} ({cron})"
        )))
    }
}

/// Delete a cron schedule by ID.
pub struct CronDeleteTool;

#[async_trait]
impl Tool for CronDeleteTool {
    fn name(&self) -> &str {
        "cron_delete"
    }
    fn description(&self) -> &str {
        "Delete a scheduled task by ID."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Schedule ID to delete" }
                },
                "required": ["id"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(id) = input.get("id").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("id required"));
        };

        // Prevent path traversal: only allow UUID-safe characters.
        if !id.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return Ok(ToolResult::error(
                "Invalid schedule ID (must be UUID format)",
            ));
        }

        let path = schedules_dir().join(format!("{id}.json"));
        if !path.exists() {
            return Ok(ToolResult::error(format!("Schedule '{id}' not found")));
        }

        std::fs::remove_file(&path)?;
        Ok(ToolResult::success(format!("Schedule '{id}' deleted")))
    }
}

/// List all cron schedules.
pub struct CronListTool;

#[async_trait]
impl Tool for CronListTool {
    fn name(&self) -> &str {
        "cron_list"
    }
    fn description(&self) -> &str {
        "List all scheduled tasks."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ToolContext,
    ) -> OxiResult<ToolResult> {
        let dir = schedules_dir();
        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&dir) {
            for entry in read_dir.flatten() {
                if entry.path().extension().is_some_and(|e| e == "json") {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if let Ok(sched) = serde_json::from_str::<ScheduleEntry>(&content) {
                            entries.push(format!(
                                "- {} | {} | {}",
                                sched.id, sched.cron, sched.command
                            ));
                        }
                    }
                }
            }
        }

        if entries.is_empty() {
            Ok(ToolResult::success("No schedules found."))
        } else {
            Ok(ToolResult::success(entries.join("\n")))
        }
    }
}
