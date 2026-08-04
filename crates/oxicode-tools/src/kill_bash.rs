//! KillBashTool — terminate a running bash process by task ID.
//!
//! Looks up the PID in the shared `BashProcessMap`, sends SIGTERM,
//! waits a grace period, then sends SIGKILL if still alive.
//! Uses `kill` CLI command to avoid unsafe libc calls.

use std::time::Duration;

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Tool for terminating running bash processes by task ID.
pub struct KillBashTool;

/// Grace period before escalating SIGTERM → SIGKILL.
const KILL_GRACE_SECS: u64 = 5;

#[async_trait]
impl Tool for KillBashTool {
    fn name(&self) -> &str {
        "kill_bash"
    }

    fn description(&self) -> &str {
        "Terminate a running bash process by its task ID. Sends SIGTERM, \
         waits 5s, then SIGKILL if still alive."
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
                        "description": "The task ID of the running bash process to kill"
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
        let task_id = input["task_id"]
            .as_str()
            .ok_or_else(|| oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: "task_id is required".into(),
            })?;

        // Look up the process in the shared bash process map.
        let process = {
            let map = ctx.bash_processes.lock().expect("lock bash_processes");
            map.get(task_id).cloned()
        };

        let Some(process) = process else {
            return Ok(ToolResult::error(format!(
                "No running bash process with task ID: {task_id}"
            )));
        };

        let pid = process.pid;
        let elapsed = process.started_at.elapsed();

        // Step 1: Send SIGTERM via kill command.
        if !send_signal(pid, "TERM") {
            // Process may have already exited — clean up and report.
            let mut map = ctx.bash_processes.lock().expect("lock bash_processes");
            map.remove(task_id);
            return Ok(ToolResult::success(format!(
                "Process {pid} (task {task_id}) already terminated. Cleaned up."
            )));
        }

        // Step 2: Wait grace period for graceful shutdown.
        tokio::time::sleep(Duration::from_secs(KILL_GRACE_SECS)).await;

        // Step 3: Check if still alive, escalate to SIGKILL if needed.
        let escalated = if is_process_alive(pid) {
            send_signal(pid, "KILL");
            // Brief wait for SIGKILL to take effect.
            tokio::time::sleep(Duration::from_millis(200)).await;
            true
        } else {
            false
        };

        // Clean up the tracking map.
        {
            let mut map = ctx.bash_processes.lock().expect("lock bash_processes");
            map.remove(task_id);
        }

        // Update TaskManager status if task is registered (background tasks only;
        // foreground tasks use fg-{uuid} IDs not registered in TaskManager).
        {
            let mut mgr = ctx.task_manager.lock().expect("lock task_manager");
            if mgr.get_task(task_id).is_some() {
                mgr.update_status(task_id, oxicode_tasks::TaskStatus::Killed);
            }
        }

        let method = if escalated { "SIGKILL" } else { "SIGTERM" };
        Ok(ToolResult::success(format!(
            "Process {pid} killed via {method} (task: {task_id}, ran for {:.1}s)",
            elapsed.as_secs_f64()
        )))
    }
}

/// Send a signal to a process via the `kill` command. Returns true if
/// the command was dispatched successfully (process existed).
fn send_signal(pid: u32, signal: &str) -> bool {
    std::process::Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Check if a process is still alive by sending signal 0.
fn is_process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Spawns a POSIX shell; oxicode's shell-backed tools are unix-only for now.
    #[cfg(unix)]
    #[test]
    fn test_is_process_alive_self() {
        // Our own process should be alive.
        let pid = std::process::id();
        assert!(is_process_alive(pid));
    }

    #[test]
    fn test_is_process_alive_nonexistent() {
        // PID 99999999 is almost certainly not alive.
        assert!(!is_process_alive(99_999_999));
    }

    #[tokio::test]
    async fn test_kill_bash_no_such_task() {
        let tool = KillBashTool;
        let ctx = ToolContext::default();

        let result = tool
            .execute(serde_json::json!({"task_id": "nonexistent-id"}), &ctx)
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("No running bash process"));
    }

    // Spawns a POSIX shell; oxicode's shell-backed tools are unix-only for now.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_kill_bash_terminates_process() {
        use crate::tool_trait::BashProcess;

        let tool = KillBashTool;
        let ctx = ToolContext::default();

        // Spawn a long-running sleep process. We intentionally do not wait — the tool
        // kills it by PID; the OS will reap it once killed.
        #[allow(clippy::zombie_processes)]
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("spawn sleep");

        let pid = child.id();
        let task_id = "test-kill-task";

        // Insert into tracking map.
        {
            let mut map = ctx.bash_processes.lock().unwrap();
            map.insert(
                task_id.to_string(),
                BashProcess {
                    pid,
                    command: "sleep 60".to_string(),
                    started_at: std::time::Instant::now(),
                },
            );
        }

        let result = tool
            .execute(serde_json::json!({"task_id": task_id}), &ctx)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("killed"));

        // Verify removed from map.
        let map = ctx.bash_processes.lock().unwrap();
        assert!(!map.contains_key(task_id));

        // Reap the (already killed) child to avoid zombie.
        let _ = child.wait();
    }
}
