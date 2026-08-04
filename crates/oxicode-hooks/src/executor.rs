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
pub async fn execute_hook(hook_def: &HookDef, payload: &HookPayload) -> HookResponse {
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
            execute_http_hook(payload, &config, None).await
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
        // A hook that never reads stdin (a bare `echo`, say) can exit before the
        // payload is fully written, which closes the pipe. That is legitimate —
        // keep going and read whatever it wrote to stdout instead of failing.
        if let Err(e) = stdin.write_all(payload_json.as_bytes()).await {
            if e.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(format!("Failed to write to hook stdin: {e}"));
            }
        }
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
    Ok(parse_hook_output(&stdout))
}

/// Parse hook stdout into a `HookResponse`.
///
/// Accepts:
/// - Legacy oxicode tagged form: `{"action":"pass"|"abort"|"modify_prompt"|"override_result"|...}`
/// - openclaude form: `{"decision":"block","reason":"..."}` → `Abort`
/// - openclaude form: `{"systemMessage":"..."}`,
///   `{"hookSpecificOutput":{"additionalContext":"..."}}`,
///   or `{"reason":"..."}` (non-fatal) → `Message`
///
/// Falls back to `HookResponse::Pass` on empty input or unparseable JSON
/// (preserves existing shell-script compatibility — most hooks emit nothing).
pub fn parse_hook_output(stdout: &str) -> HookResponse {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return HookResponse::Pass;
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return HookResponse::Pass;
    };

    // Legacy tagged form first — preserves current behavior for existing hooks.
    if value.get("action").is_some() {
        if let Ok(resp) = serde_json::from_value::<HookResponse>(value.clone()) {
            return resp;
        }
    }

    // openclaude-style block decision.
    if value.get("decision").and_then(|v| v.as_str()) == Some("block") {
        let reason = value
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("blocked")
            .to_string();
        return HookResponse::Abort { reason };
    }

    let system_message = value
        .get("systemMessage")
        .and_then(|v| v.as_str())
        .map(String::from);
    let additional_context = value
        .pointer("/hookSpecificOutput/additionalContext")
        .and_then(|v| v.as_str())
        .map(String::from);
    // Only treat top-level `reason` as a non-fatal message when no decision tag set.
    let block_reason = value
        .get("reason")
        .and_then(|v| v.as_str())
        .map(String::from);

    if system_message.is_some() || additional_context.is_some() || block_reason.is_some() {
        return HookResponse::Message {
            system_message,
            additional_context,
            block_reason,
        };
    }

    HookResponse::Pass
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::HookEvent;

    fn test_payload() -> HookPayload {
        HookPayload::new(HookEvent::SessionStart, "test-session")
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
        let mut payload = HookPayload::new(HookEvent::PreToolUse, "test-session");
        payload.tool_name = Some("bash".to_string());
        payload.tool_input = Some(serde_json::json!({"command": "ls"}));
        let response = execute_hook_script(
            r#"input=$(cat); echo "$input" | grep -q "PreToolUse" && echo '{"action":"pass"}' || echo '{"action":"abort","reason":"missing event"}'"#,
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

    // -- parse_hook_output unit tests --

    #[test]
    fn parse_empty_returns_pass() {
        assert!(matches!(parse_hook_output(""), HookResponse::Pass));
        assert!(matches!(parse_hook_output("   \n"), HookResponse::Pass));
    }

    #[test]
    fn parse_plain_text_returns_pass() {
        assert!(matches!(
            parse_hook_output("hello world"),
            HookResponse::Pass
        ));
    }

    #[test]
    fn parse_legacy_action_abort() {
        let resp = parse_hook_output(r#"{"action":"abort","reason":"x"}"#);
        assert!(matches!(resp, HookResponse::Abort { reason } if reason == "x"));
    }

    #[test]
    fn parse_openclaude_decision_block() {
        let resp = parse_hook_output(r#"{"decision":"block","reason":"nope"}"#);
        assert!(matches!(resp, HookResponse::Abort { reason } if reason == "nope"));
    }

    #[test]
    fn parse_openclaude_system_message() {
        let resp = parse_hook_output(r#"{"systemMessage":"hi"}"#);
        match resp {
            HookResponse::Message {
                system_message,
                additional_context,
                block_reason,
            } => {
                assert_eq!(system_message.as_deref(), Some("hi"));
                assert!(additional_context.is_none());
                assert!(block_reason.is_none());
            }
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn parse_openclaude_additional_context() {
        let resp = parse_hook_output(r#"{"hookSpecificOutput":{"additionalContext":"some ctx"}}"#);
        match resp {
            HookResponse::Message {
                additional_context, ..
            } => {
                assert_eq!(additional_context.as_deref(), Some("some ctx"));
            }
            other => panic!("expected Message, got {other:?}"),
        }
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
