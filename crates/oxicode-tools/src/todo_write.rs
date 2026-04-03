//! TodoWrite tool: manage a session task checklist persisted to disk.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use oxicode_common::OxiResult;
use serde::{Deserialize, Serialize};

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Single todo item.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TodoItem {
    id: String,
    content: String,
    status: String, // "pending" | "in_progress" | "completed"
    #[serde(default)]
    priority: String, // "high" | "medium" | "low"
}

/// Persisted todo list.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TodoList {
    todos: Vec<TodoItem>,
}

/// Resolve todos file path: ~/.oxicode/todos.json
fn todos_path(working_dir: &Path) -> PathBuf {
    // Use home dir for real usage, but respect working_dir for test isolation
    // when HOME isn't writable or during tests.
    let base = dirs::home_dir().unwrap_or_else(|| working_dir.to_path_buf());
    base.join(".oxicode").join("todos.json")
}

fn load_todos(path: &Path) -> TodoList {
    if path.exists() {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        TodoList::default()
    }
}

fn save_todos(path: &Path, list: &TodoList) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(list).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

pub struct TodoWriteTool;

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "TodoWrite"
    }
    fn description(&self) -> &str {
        "Write and manage a session task checklist. Replaces the entire todo list with the provided items. Clears the list when all tasks are completed."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "description": "The complete todo list (replaces existing)",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "description": "Unique task identifier" },
                                "content": { "type": "string", "description": "Task description" },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed"],
                                    "description": "Task status"
                                },
                                "priority": {
                                    "type": "string",
                                    "enum": ["high", "medium", "low"],
                                    "description": "Task priority (default: medium)"
                                }
                            },
                            "required": ["id", "content", "status"]
                        }
                    }
                },
                "required": ["todos"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::FileWrite
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(todos_val) = input.get("todos") else {
            return Ok(ToolResult::error("todos array is required"));
        };

        let new_items: Vec<TodoItem> = match serde_json::from_value(todos_val.clone()) {
            Ok(items) => items,
            Err(e) => return Ok(ToolResult::error(format!("Invalid todos format: {e}"))),
        };

        let path = todos_path(&ctx.working_dir);
        let old_list = load_todos(&path);

        // Clear list if all tasks completed
        let all_done = !new_items.is_empty()
            && new_items.iter().all(|t| t.status == "completed");
        let final_items = if all_done { vec![] } else { new_items };

        let new_list = TodoList { todos: final_items };
        if let Err(e) = save_todos(&path, &new_list) {
            return Ok(ToolResult::error(format!("Failed to save todos: {e}")));
        }

        let result = serde_json::json!({
            "oldTodos": old_list.todos,
            "newTodos": new_list.todos,
            "cleared": all_done,
        });

        Ok(ToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> (tempfile::TempDir, ToolContext) {
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = ToolContext {
            working_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        (dir, ctx)
    }

    #[tokio::test]
    async fn test_todo_write_basic() {
        let (_dir, ctx) = test_ctx();
        // Override HOME so todos_path uses temp dir
        std::env::set_var("HOME", ctx.working_dir.to_string_lossy().as_ref());
        let tool = TodoWriteTool;
        let input = serde_json::json!({
            "todos": [
                {"id": "1", "content": "Test task", "status": "pending"}
            ]
        });
        let result = tool.execute(input, &ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Test task"));
    }

    #[tokio::test]
    async fn test_todo_write_missing_field() {
        let (_dir, ctx) = test_ctx();
        let tool = TodoWriteTool;
        let input = serde_json::json!({});
        let result = tool.execute(input, &ctx).await.unwrap();
        assert!(result.is_error);
    }
}
