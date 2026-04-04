//! PowerShell tool: execute PowerShell commands via `pwsh` or `powershell.exe`.
//!
//! Mirrors BashTool patterns (timeout, output caps, kill-on-drop) but targets
//! PowerShell on all platforms.

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use oxicode_common::OxiResult;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
const MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024;

pub struct PowerShellTool;

/// Detect the PowerShell executable available on this system.
fn powershell_exe() -> &'static str {
    if cfg!(target_os = "windows") {
        // Prefer pwsh (PowerShell 7+), fall back to powershell.exe (Windows PowerShell 5.1)
        if which_exists("pwsh") {
            "pwsh"
        } else {
            "powershell.exe"
        }
    } else {
        "pwsh"
    }
}

fn which_exists(name: &str) -> bool {
    let cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    std::process::Command::new(cmd)
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[async_trait]
impl Tool for PowerShellTool {
    fn name(&self) -> &str {
        "PowerShell"
    }
    fn description(&self) -> &str {
        "Execute a PowerShell command and return its output. Uses pwsh (cross-platform) or powershell.exe (Windows)."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The PowerShell command to execute"
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in milliseconds (default: 120000, max: 600000)"
                    },
                    "description": {
                        "type": "string",
                        "description": "Clear, concise description of what this command does"
                    }
                },
                "required": ["command"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ShellExec
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(command) = input.get("command").and_then(serde_json::Value::as_str) else {
            return Ok(ToolResult::error("command is required"));
        };

        let timeout_ms = input
            .get("timeout")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS);

        let exe = powershell_exe();

        let result = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            run_powershell(exe, command, &ctx.working_dir),
        )
        .await;

        match result {
            Ok(Ok((stdout, stderr, code))) => {
                let mut output = String::new();
                if !stdout.is_empty() {
                    output.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(&stderr);
                }
                if output.is_empty() {
                    output = "(no output)".to_string();
                }
                if code == 0 {
                    Ok(ToolResult::success(output))
                } else {
                    Ok(ToolResult::error(format!("Exit code: {code}\n{output}")))
                }
            }
            Ok(Err(e)) => Ok(ToolResult::error(format!(
                "Failed to execute PowerShell ('{exe}'): {e}. Is pwsh installed?"
            ))),
            Err(_) => Ok(ToolResult::error(format!(
                "Command timed out after {timeout_ms}ms"
            ))),
        }
    }
}

async fn run_powershell(
    exe: &str,
    command: &str,
    working_dir: &Path,
) -> Result<(String, String, i32), std::io::Error> {
    let mut child = Command::new(exe)
        .args(["-NoProfile", "-NonInteractive", "-Command", command])
        .current_dir(working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout_pipe = child.stdout.take().expect("stdout piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr piped");

    let stdout_handle = tokio::spawn(async move {
        let mut buf = Vec::with_capacity(8192);
        let mut tmp = [0u8; 8192];
        loop {
            match stdout_pipe.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let remaining = MAX_OUTPUT_BYTES.saturating_sub(buf.len());
                    buf.extend_from_slice(&tmp[..n.min(remaining)]);
                    if buf.len() >= MAX_OUTPUT_BYTES {
                        break;
                    }
                }
            }
        }
        buf
    });

    let stderr_cap = MAX_OUTPUT_BYTES / 4;
    let stderr_handle = tokio::spawn(async move {
        let mut buf = Vec::with_capacity(4096);
        let mut tmp = [0u8; 4096];
        loop {
            match stderr_pipe.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let remaining = stderr_cap.saturating_sub(buf.len());
                    buf.extend_from_slice(&tmp[..n.min(remaining)]);
                    if buf.len() >= stderr_cap {
                        break;
                    }
                }
            }
        }
        buf
    });

    let stdout_bytes = stdout_handle.await.unwrap_or_default();
    let stderr_bytes = stderr_handle.await.unwrap_or_default();
    let status = child.wait().await?;

    let mut stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
    let mut stderr = String::from_utf8_lossy(&stderr_bytes).to_string();

    if stdout_bytes.len() >= MAX_OUTPUT_BYTES {
        stdout.push_str("\n... (output truncated at 10 MB)");
    }
    if stderr_bytes.len() >= stderr_cap {
        stderr.push_str("\n... (stderr truncated at 2.5 MB)");
    }

    let code = status.code().unwrap_or(-1);
    Ok((stdout, stderr, code))
}
