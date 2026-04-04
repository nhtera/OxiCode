use async_trait::async_trait;
use oxicode_common::OxiResult;
use oxicode_tasks::{TaskStatus, TaskType};

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Lock a mutex, recovering from poison (the data is still usable).
fn lock_mutex<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Truncate a string to at most `max_bytes` bytes, appending "..." if truncated.
/// Always cuts at a valid UTF-8 char boundary to avoid panics on multi-byte input.
fn truncate_desc(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.saturating_sub(3);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

// ---------------------------------------------------------------------------
// TaskCreate
// ---------------------------------------------------------------------------

/// Create and spawn a background task (bash command or agent prompt).
pub struct TaskCreateTool;

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        "task_create"
    }
    fn description(&self) -> &str {
        "Create a background task (bash command or agent) that runs asynchronously."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "type": {
                        "type": "string",
                        "enum": ["bash", "agent"],
                        "description": "Task type: 'bash' for shell commands, 'agent' for LLM agent"
                    },
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute (required for bash type)"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Agent prompt (required for agent type)"
                    },
                    "model": {
                        "type": "string",
                        "description": "Model for agent tasks (default: claude-sonnet-4-20250514)"
                    }
                },
                "required": ["type"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(task_type_str) = input["type"].as_str() else {
            return Ok(ToolResult::error("'type' is required (bash or agent)"));
        };

        let (task_type, description) = match task_type_str {
            "bash" => {
                let Some(cmd) = input["command"].as_str().map(String::from) else {
                    return Ok(ToolResult::error("'command' required for bash tasks"));
                };
                let desc = truncate_desc(&cmd, 60);
                (TaskType::LocalBash { command: cmd }, desc)
            }
            "agent" => {
                let Some(prompt) = input["prompt"].as_str().map(String::from) else {
                    return Ok(ToolResult::error("'prompt' required for agent tasks"));
                };
                let model = input["model"]
                    .as_str()
                    .unwrap_or("claude-sonnet-4-20250514")
                    .to_string();
                let desc = truncate_desc(&prompt, 60);
                (TaskType::LocalAgent { prompt, model }, desc)
            }
            other => {
                return Ok(ToolResult::error(format!(
                    "Unknown task type '{other}'. Use 'bash' or 'agent'."
                )));
            }
        };

        // Create entry in manager.
        let (task_id, tasks_dir) = {
            let mut mgr = lock_mutex(&ctx.task_manager);
            let id = mgr.create_task(task_type.clone());
            mgr.update_status(&id, TaskStatus::Running);
            (id, mgr.tasks_dir.clone())
        };

        // Spawn background execution.
        let mgr_ref = ctx.task_manager.clone();
        let abort_map = ctx.task_abort_handles.clone();
        let tid = task_id.clone();

        let handle = tokio::spawn(async move {
            let status = match &task_type {
                TaskType::LocalBash { command } => {
                    oxicode_tasks::runner::run_bash(&tid, command, &tasks_dir)
                        .await
                        .unwrap_or_else(|e| TaskStatus::Failed {
                            error: e.to_string(),
                        })
                }
                TaskType::LocalAgent { prompt, model } => {
                    oxicode_tasks::runner::run_agent(&tid, prompt, model, &tasks_dir)
                        .await
                        .unwrap_or_else(|e| TaskStatus::Failed {
                            error: e.to_string(),
                        })
                }
                TaskType::Monitor { .. } => TaskStatus::Failed {
                    error: "monitor tasks not yet supported".into(),
                },
                #[cfg(feature = "remote")]
                TaskType::RemoteAgent { .. } => TaskStatus::Failed {
                    error: "remote agent tasks: use run_remote_agent directly".into(),
                },
                #[cfg(feature = "teammate")]
                TaskType::InProcessTeammate { .. } => TaskStatus::Failed {
                    error: "teammate tasks: use run_teammate directly".into(),
                },
                #[cfg(feature = "dream")]
                TaskType::Dream { .. } => TaskStatus::Failed {
                    error: "dream tasks: use run_dream directly".into(),
                },
            };
            // Update final status (recover from poison — inside spawn, can't propagate).
            if let Ok(mut mgr) = mgr_ref.lock() {
                mgr.update_status(&tid, status);
            } else {
                tracing::error!("Task manager lock poisoned, cannot update task {tid}");
            }
            // Remove abort handle (task finished naturally).
            if let Ok(mut handles) = abort_map.lock() {
                handles.remove(&tid);
            } else {
                tracing::error!("Abort handles lock poisoned for task {tid}");
            }
        });

        // Store abort handle for TaskStop.
        lock_mutex(&ctx.task_abort_handles).insert(task_id.clone(), handle.abort_handle());

        Ok(ToolResult::success(format!(
            "Task {task_id} created and running: {description}"
        )))
    }
}

// ---------------------------------------------------------------------------
// TaskGet
// ---------------------------------------------------------------------------

/// Retrieve details of a single task by ID.
pub struct TaskGetTool;

#[async_trait]
impl Tool for TaskGetTool {
    fn name(&self) -> &str {
        "task_get"
    }
    fn description(&self) -> &str {
        "Get details of a background task by its ID."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "The task ID to look up"
                    }
                },
                "required": ["task_id"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(task_id) = input["task_id"].as_str() else {
            return Ok(ToolResult::error("'task_id' is required"));
        };

        let mgr = lock_mutex(&ctx.task_manager);
        match mgr.get_task(task_id) {
            Some(entry) => {
                let json = serde_json::to_string_pretty(entry).unwrap_or_default();
                Ok(ToolResult::success(json))
            }
            None => Ok(ToolResult::error(format!("Task '{task_id}' not found"))),
        }
    }
}

// ---------------------------------------------------------------------------
// TaskList
// ---------------------------------------------------------------------------

/// List all background tasks with their status.
pub struct TaskListTool;

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &str {
        "task_list"
    }
    fn description(&self) -> &str {
        "List all background tasks and their statuses."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, _input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let mgr = lock_mutex(&ctx.task_manager);
        let tasks = mgr.list_tasks();

        if tasks.is_empty() {
            return Ok(ToolResult::success("No background tasks."));
        }

        let json = serde_json::to_string_pretty(&tasks).unwrap_or_default();
        Ok(ToolResult::success(json))
    }
}

// ---------------------------------------------------------------------------
// TaskUpdate
// ---------------------------------------------------------------------------

/// Manually update the status of a task.
pub struct TaskUpdateTool;

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        "task_update"
    }
    fn description(&self) -> &str {
        "Update the status of a background task."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "The task ID to update"
                    },
                    "status": {
                        "type": "string",
                        "enum": ["pending", "running", "completed", "failed", "killed"],
                        "description": "New status for the task"
                    }
                },
                "required": ["task_id", "status"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(task_id) = input["task_id"].as_str() else {
            return Ok(ToolResult::error("'task_id' is required"));
        };
        let Some(status_str) = input["status"].as_str() else {
            return Ok(ToolResult::error("'status' is required"));
        };

        let status = match status_str {
            "pending" => TaskStatus::Pending,
            "running" => TaskStatus::Running,
            "completed" => TaskStatus::Completed { exit_code: 0 },
            "failed" => TaskStatus::Failed {
                error: "manually set to failed".into(),
            },
            "killed" => TaskStatus::Killed,
            other => {
                return Ok(ToolResult::error(format!("Unknown status '{other}'")));
            }
        };

        let mut mgr = lock_mutex(&ctx.task_manager);
        if mgr.get_task(task_id).is_none() {
            return Ok(ToolResult::error(format!("Task '{task_id}' not found")));
        }
        mgr.update_status(task_id, status);

        Ok(ToolResult::success(format!(
            "Task {task_id} status updated to {status_str}"
        )))
    }
}

// ---------------------------------------------------------------------------
// TaskStop
// ---------------------------------------------------------------------------

/// Stop a running background task.
pub struct TaskStopTool;

#[async_trait]
impl Tool for TaskStopTool {
    fn name(&self) -> &str {
        "task_stop"
    }
    fn description(&self) -> &str {
        "Stop a running background task by aborting its process."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "The task ID to stop"
                    }
                },
                "required": ["task_id"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(task_id) = input["task_id"].as_str() else {
            return Ok(ToolResult::error("'task_id' is required"));
        };

        // Hold both locks to make check+abort+update atomic.
        // Lock ordering: task_manager first, then abort_handles (always).
        let mut mgr = lock_mutex(&ctx.task_manager);
        match mgr.get_task(task_id) {
            None => return Ok(ToolResult::error(format!("Task '{task_id}' not found"))),
            Some(entry) => {
                if !matches!(entry.status, TaskStatus::Running) {
                    return Ok(ToolResult::error(format!(
                        "Task '{task_id}' is not running"
                    )));
                }
            }
        }

        // Abort the background tokio task.
        let aborted = lock_mutex(&ctx.task_abort_handles)
            .remove(task_id)
            .is_some_and(|h| {
                h.abort();
                true
            });

        // Update status while still holding the manager lock.
        mgr.update_status(task_id, TaskStatus::Killed);

        if aborted {
            Ok(ToolResult::success(format!("Task {task_id} stopped")))
        } else {
            Ok(ToolResult::success(format!(
                "Task {task_id} marked as killed (process may have already exited)"
            )))
        }
    }
}

// ---------------------------------------------------------------------------
// TaskOutput
// ---------------------------------------------------------------------------

/// Read output from a background task.
pub struct TaskOutputTool;

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str {
        "task_output"
    }
    fn description(&self) -> &str {
        "Read stdout/stderr output from a background task."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "The task ID whose output to read"
                    },
                    "tail": {
                        "type": "integer",
                        "description": "Return only the last N lines (default: all)"
                    }
                },
                "required": ["task_id"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(task_id) = input["task_id"].as_str() else {
            return Ok(ToolResult::error("'task_id' is required"));
        };
        let tail = input["tail"].as_u64();

        // Single lock: get tasks_dir and check existence.
        let tasks_dir = {
            let mgr = lock_mutex(&ctx.task_manager);
            if mgr.get_task(task_id).is_none() {
                return Ok(ToolResult::error(format!("Task '{task_id}' not found")));
            }
            mgr.tasks_dir.clone()
        };

        let lines = oxicode_tasks::output::read_all(task_id, &tasks_dir).map_err(|e| {
            oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: format!("Failed to read output: {e}"),
            }
        })?;

        if lines.is_empty() {
            return Ok(ToolResult::success("(no output yet)"));
        }

        let display: Vec<_> = if let Some(n) = tail {
            let skip = lines.len().saturating_sub(n as usize);
            lines[skip..].to_vec()
        } else {
            lines
        };

        let mut output = String::new();
        for line in &display {
            let prefix = match line.stream {
                oxicode_tasks::output::OutputStream::Stdout => "",
                oxicode_tasks::output::OutputStream::Stderr => "[stderr] ",
            };
            output.push_str(prefix);
            output.push_str(&line.line);
            output.push('\n');
        }

        Ok(ToolResult::success(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_task_create_bash() {
        let ctx = ToolContext::default();
        let tool = TaskCreateTool;

        let result = tool
            .execute(
                serde_json::json!({"type": "bash", "command": "echo hello"}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("created and running"));

        // Wait for task to finish.
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify task completed.
        let mgr = ctx.task_manager.lock().unwrap();
        let tasks = mgr.list_tasks();
        assert_eq!(tasks.len(), 1);
        assert!(matches!(
            tasks[0].status,
            TaskStatus::Completed { exit_code: 0 }
        ));
    }

    #[tokio::test]
    async fn test_task_list_empty() {
        let ctx = ToolContext::default();
        let tool = TaskListTool;

        let result = tool.execute(serde_json::json!({}), &ctx).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("No background tasks"));
    }

    #[tokio::test]
    async fn test_task_get_not_found() {
        let ctx = ToolContext::default();
        let tool = TaskGetTool;

        let result = tool
            .execute(serde_json::json!({"task_id": "nonexistent"}), &ctx)
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("not found"));
    }

    #[tokio::test]
    async fn test_task_create_and_get() {
        let ctx = ToolContext::default();

        // Create a quick task.
        let create = TaskCreateTool;
        let result = create
            .execute(
                serde_json::json!({"type": "bash", "command": "echo test"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.is_error);

        // Extract task ID from the response.
        let task_id = {
            let mgr = ctx.task_manager.lock().unwrap();
            mgr.list_tasks()[0].id.clone()
        };

        // Get the task.
        let get = TaskGetTool;
        let result = get
            .execute(serde_json::json!({"task_id": task_id}), &ctx)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains(&task_id));
    }

    #[tokio::test]
    async fn test_task_stop() {
        let ctx = ToolContext::default();

        // Start a long-running task.
        let create = TaskCreateTool;
        create
            .execute(
                serde_json::json!({"type": "bash", "command": "sleep 60"}),
                &ctx,
            )
            .await
            .unwrap();

        let task_id = {
            let mgr = ctx.task_manager.lock().unwrap();
            mgr.list_tasks()[0].id.clone()
        };

        // Give it a moment to start.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Stop it.
        let stop = TaskStopTool;
        let result = stop
            .execute(serde_json::json!({"task_id": task_id}), &ctx)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("stopped"));

        // Verify status is Killed.
        let mgr = ctx.task_manager.lock().unwrap();
        let entry = mgr.get_task(&task_id).unwrap();
        assert!(matches!(entry.status, TaskStatus::Killed));
    }

    #[tokio::test]
    async fn test_task_output() {
        let ctx = ToolContext::default();

        // Create a task that produces output.
        let create = TaskCreateTool;
        create
            .execute(
                serde_json::json!({"type": "bash", "command": "echo line1 && echo line2 && echo line3"}),
                &ctx,
            )
            .await
            .unwrap();

        // Wait for it to finish.
        tokio::time::sleep(Duration::from_millis(500)).await;

        let task_id = {
            let mgr = ctx.task_manager.lock().unwrap();
            mgr.list_tasks()[0].id.clone()
        };

        // Read all output.
        let output_tool = TaskOutputTool;
        let result = output_tool
            .execute(serde_json::json!({"task_id": task_id}), &ctx)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("line1"));
        assert!(result.content.contains("line3"));

        // Read only last 2 lines.
        let result = output_tool
            .execute(serde_json::json!({"task_id": task_id, "tail": 2}), &ctx)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(!result.content.contains("line1"));
        assert!(result.content.contains("line2"));
        assert!(result.content.contains("line3"));
    }

    #[tokio::test]
    async fn test_task_update_status() {
        let ctx = ToolContext::default();

        // Create task.
        let create = TaskCreateTool;
        create
            .execute(
                serde_json::json!({"type": "bash", "command": "sleep 60"}),
                &ctx,
            )
            .await
            .unwrap();

        let task_id = {
            let mgr = ctx.task_manager.lock().unwrap();
            mgr.list_tasks()[0].id.clone()
        };

        // Update to failed.
        let update = TaskUpdateTool;
        let result = update
            .execute(
                serde_json::json!({"task_id": task_id, "status": "failed"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("updated to failed"));

        // Clean up — abort the background task.
        if let Some(h) = ctx.task_abort_handles.lock().unwrap().remove(&task_id) {
            h.abort();
        };
    }

    #[tokio::test]
    async fn test_task_create_missing_command() {
        let ctx = ToolContext::default();
        let tool = TaskCreateTool;

        let result = tool
            .execute(serde_json::json!({"type": "bash"}), &ctx)
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("'command' required"));
    }
}
