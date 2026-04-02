//! Hook subprocess executor.
//!
//! Spawns shell scripts with JSON payload on stdin, reads JSON response from stdout.
//! Enforces timeout (default 10s) and handles errors gracefully.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::events::{HookPayload, HookResponse};

/// Default timeout for hook subprocess execution.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Execute a hook script as a subprocess.
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
}
