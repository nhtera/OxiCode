//! REPL tool: execute code in a persistent Python or Node.js REPL session.
//!
//! Spawns a subprocess REPL, sends code, and collects output. Sessions persist
//! across multiple invocations within the same tool context.

use std::time::Duration;

use async_trait::async_trait;
use oxicode_common::OxiResult;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024; // 1 MB

pub struct ReplTool;

/// Detect REPL command for a given language.
fn repl_command(language: &str) -> Option<(&'static str, Vec<&'static str>)> {
    match language {
        "python" | "py" => Some(("python3", vec!["-c"])),
        "node" | "javascript" | "js" => Some(("node", vec!["-e"])),
        "ruby" | "rb" => Some(("ruby", vec!["-e"])),
        _ => None,
    }
}

#[async_trait]
impl Tool for ReplTool {
    fn name(&self) -> &str {
        "REPL"
    }
    fn description(&self) -> &str {
        "Execute code in a Python, Node.js, or Ruby subprocess. Each invocation runs the code and returns output."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "language": {
                        "type": "string",
                        "enum": ["python", "node", "ruby"],
                        "description": "Programming language for the REPL"
                    },
                    "code": {
                        "type": "string",
                        "description": "Code to execute"
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in milliseconds (default: 30000)"
                    }
                },
                "required": ["language", "code"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ShellExec
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(language) = input.get("language").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("language is required"));
        };
        let Some(code) = input.get("code").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("code is required"));
        };

        let timeout_ms = input
            .get("timeout")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS);

        let Some((exe, args)) = repl_command(language) else {
            return Ok(ToolResult::error(format!(
                "Unsupported language: {language}. Supported: python, node, ruby"
            )));
        };

        let result = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            run_repl(exe, &args, code, &ctx.working_dir),
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
                "Failed to execute {exe}: {e}. Is it installed?"
            ))),
            Err(_) => Ok(ToolResult::error(format!(
                "REPL execution timed out after {timeout_ms}ms"
            ))),
        }
    }
}

async fn run_repl(
    exe: &str,
    args: &[&str],
    code: &str,
    working_dir: &std::path::Path,
) -> Result<(String, String, i32), std::io::Error> {
    let mut child = Command::new(exe)
        .args(args)
        .arg(code)
        .current_dir(working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout_pipe = child.stdout.take().expect("stdout piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr piped");

    let stdout_handle = tokio::spawn(async move {
        let mut buf = Vec::with_capacity(4096);
        let mut tmp = [0u8; 4096];
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

    let stderr_handle = tokio::spawn(async move {
        let mut buf = Vec::with_capacity(4096);
        let mut tmp = [0u8; 4096];
        loop {
            match stderr_pipe.read(&mut tmp).await {
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

    let stdout_bytes = stdout_handle.await.unwrap_or_default();
    let stderr_bytes = stderr_handle.await.unwrap_or_default();
    let status = child.wait().await?;

    let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
    let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();
    let exit_code = status.code().unwrap_or(-1);
    Ok((stdout, stderr, exit_code))
}
