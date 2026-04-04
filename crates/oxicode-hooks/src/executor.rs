//! Hook executor — dispatches to command (subprocess), agent (LLM), or HTTP based on type.
//!
//! Spawns shell scripts with JSON payload on stdin, reads JSON response from stdout.
//! Enforces timeout (default 10s) and handles errors gracefully.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::agent_hook_executor::execute_agent_hook;
use crate::config::{HookDef, HookType};
use crate::events::{HookPayload, HookResponse};
use crate::http_hook_executor::execute_http_hook;

/// Default timeout for hook subprocess execution.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Execute a hook based on its type (command, agent, or http).
///
/// Dispatches to the appropriate executor:
/// - `Command` → subprocess with JSON stdin/stdout
/// - `Agent` → LLM call with structured output
/// - `Http` → HTTP POST to URL endpoint
///
/// Returns `HookResponse::Pass` on any failure (fail-open).
pub async fn execute_hook(
    hook_def: &HookDef,
    payload: &HookPayload,
) -> HookResponse {
    match hook_def.hook_type {
        HookType::Command => {
            let timeout = Duration::from_secs(hook_def.timeout_secs);
            execute_hook_script(&hook_def.command, payload, Some(timeout)).await
        }
        HookType::Agent => {
            let config = hook_def.agent_config();
            execute_agent_hook(payload, &config).await
        }
        HookType::Http => {
            let config = hook_def.http_config();
            execute_http_hook(payload, &config).await
        }
    }
}

/// Execute a hook script as a subprocess (Command type).
///
/// - Sends `payload` as JSON on stdin
/// - Reads JSON `HookResponse` from stdout
/// - Kills the process if it exceeds `timeout`
/// - Returns `HookResponse::Pass` on any failure (hooks should not block the app)
pub async fn execute_hook_script(
    command: &str,
    payload: &HookPayload,
    timeout: Option<Duration>,
) -> HookResponse {
    let timeout = timeout.unwrap_or(DEFAULT_TIMEOUT);

    let payload_json = match serde_json::to_string(payload) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!("Failed to serialize hook payload: {e}");
            return HookResponse::Pass;
        }
    };

    let result = tokio::time::timeout(timeout, run_subprocess(command, &payload_json)).await;

    match result {
        Ok(Ok(response)) => response,
        Ok(Err(e)) => {
            tracing::warn!("Hook script error: {e}");
            HookResponse::Pass
        }
        Err(_) => {
            tracing::warn!("Hook script timed out after {timeout:?}: {command}");
            HookResponse::Pass
        }
    }
}

/// Spawn subprocess, write payload to stdin, read response from stdout.
async fn run_subprocess(command: &str, payload_json: &str) -> Result<HookResponse, String> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn hook: {e}"))?;

    // Write payload to stdin.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload_json.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to hook stdin: {e}"))?;
        // Drop stdin to signal EOF.
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("Failed to wait for hook: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::debug!("Hook exited with {}: {stderr}", output.status);
        // Non-zero exit = pass (don't block the app).
        return Ok(HookResponse::Pass);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    if trimmed.is_empty() {
        // No output = pass.
        return Ok(HookResponse::Pass);
    }

    serde_json::from_str(trimmed).map_err(|e| format!("Failed to parse hook response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::HookEvent;

    fn test_payload() -> HookPayload {
        HookPayload {
            event: HookEvent::SessionStart,
            data: serde_json::json!({}),
            session_id: None,
            model: None,
        }
    }

    #[tokio::test]
    async fn test_echo_pass_hook() {
        let response = execute_hook_script(
            r#"echo '{"action":"pass"}'"#,
            &test_payload(),
            Some(Duration::from_secs(5)),
        )
        .await;
        assert!(matches!(response, HookResponse::Pass));
    }

    #[tokio::test]
    async fn test_empty_output_is_pass() {
        let response = execute_hook_script("cat > /dev/null", &test_payload(), None).await;
        assert!(matches!(response, HookResponse::Pass));
    }

    #[tokio::test]
    async fn test_timeout_returns_pass() {
        let response = execute_hook_script(
            "sleep 30",
            &test_payload(),
            Some(Duration::from_millis(100)),
        )
        .await;
        assert!(matches!(response, HookResponse::Pass));
    }

    #[tokio::test]
    async fn test_modify_prompt_response() {
        let response = execute_hook_script(
            r#"echo '{"action":"modify_prompt","text":"injected"}'"#,
            &test_payload(),
            None,
        )
        .await;
        assert!(matches!(response, HookResponse::ModifyPrompt { text } if text == "injected"));
    }

    #[tokio::test]
    async fn test_nonzero_exit_is_pass() {
        let response = execute_hook_script("exit 1", &test_payload(), None).await;
        assert!(matches!(response, HookResponse::Pass));
    }

    #[tokio::test]
    async fn test_abort_response() {
        let response = execute_hook_script(
            r#"echo '{"action":"abort","reason":"policy violation"}'"#,
            &test_payload(),
            None,
        )
        .await;
        assert!(matches!(response, HookResponse::Abort { reason } if reason == "policy violation"));
    }

    #[tokio::test]
    async fn test_override_result_response() {
        let response = execute_hook_script(
            r#"echo '{"action":"override_result","text":"replaced output"}'"#,
            &test_payload(),
            None,
        )
        .await;
        assert!(
            matches!(response, HookResponse::OverrideResult { text } if text == "replaced output")
        );
    }

    #[tokio::test]
    async fn test_invalid_json_returns_pass() {
        let response = execute_hook_script("echo 'not valid json'", &test_payload(), None).await;
        assert!(matches!(response, HookResponse::Pass));
    }

    #[tokio::test]
    async fn test_payload_received_by_script() {
        let payload = HookPayload {
            event: HookEvent::ToolCallBefore,
            data: serde_json::json!({"tool": "bash"}),
            session_id: Some("test-session".to_string()),
            model: None,
        };
        let response = execute_hook_script(
            r#"input=$(cat); echo "$input" | grep -q "tool_call_before" && echo '{"action":"pass"}' || echo '{"action":"abort","reason":"missing event"}'"#,
            &payload,
            Some(Duration::from_secs(5)),
        )
        .await;
        assert!(matches!(response, HookResponse::Pass));
    }

    #[tokio::test]
    async fn test_invalid_command_returns_pass() {
        let response = execute_hook_script(
            "/nonexistent/command/path",
            &test_payload(),
            Some(Duration::from_secs(2)),
        )
        .await;
        assert!(matches!(response, HookResponse::Pass));
    }

    #[tokio::test]
    async fn test_stderr_does_not_affect_result() {
        let response = execute_hook_script(
            r#"echo "warning" >&2; echo '{"action":"pass"}'"#,
            &test_payload(),
            None,
        )
        .await;
        assert!(matches!(response, HookResponse::Pass));
    }

    // -- Dispatch tests --

    #[tokio::test]
    async fn test_execute_hook_command_type() {
        let hook_def = HookDef {
            hook_type: HookType::Command,
            command: r#"echo '{"action":"pass"}'"#.to_string(),
            timeout_secs: 5,
            enabled: true,
            agent: None,
            http: None,
            instructions: None,
            model: None,
            url: None,
            authorization: None,
        };
        let response = execute_hook(&hook_def, &test_payload()).await;
        assert!(matches!(response, HookResponse::Pass));
    }

    #[tokio::test]
    async fn test_execute_hook_agent_type() {
        let hook_def = HookDef {
            hook_type: HookType::Agent,
            command: String::new(),
            timeout_secs: 5,
            enabled: true,
            agent: None,
            http: None,
            instructions: Some("Test instructions".to_string()),
            model: None,
            url: None,
            authorization: None,
        };
        // Agent stub returns Pass.
        let response = execute_hook(&hook_def, &test_payload()).await;
        assert!(matches!(response, HookResponse::Pass));
    }

    #[tokio::test]
    async fn test_execute_hook_http_type_ssrf_blocked() {
        let hook_def = HookDef {
            hook_type: HookType::Http,
            command: String::new(),
            timeout_secs: 5,
            enabled: true,
            agent: None,
            http: None,
            instructions: None,
            model: None,
            url: Some("http://127.0.0.1:9999/hook".to_string()),
            authorization: None,
        };
        // SSRF blocks localhost → Pass.
        let response = execute_hook(&hook_def, &test_payload()).await;
        assert!(matches!(response, HookResponse::Pass));
    }
}
