//! Remote agent task — connects to a remote `oxicode --server` instance.
//!
//! Sends a prompt over HTTP, streams the response back to the local output file,
//! and handles connection failures with retry logic.

use std::path::Path;
use std::time::Duration;

use oxicode_common::{OxiError, OxiResult};
use serde_json::json;

use crate::manager::TaskStatus;
use crate::task_output_helpers::{open_output_file, write_line};

/// Maximum number of connection retries before giving up.
const MAX_RETRIES: u32 = 3;

/// Delay between retries (doubles each attempt).
const BASE_RETRY_DELAY_MS: u64 = 1000;

/// Default timeout for the HTTP request.
const REQUEST_TIMEOUT_SECS: u64 = 300;

/// Connect to a remote oxicode server, send a prompt, and stream the response to disk.
///
/// The remote server is expected to accept POST `{server_url}/v1/prompt` with JSON body
/// `{ "prompt": "...", "model": "..." }` and return a streaming NDJSON response.
pub async fn run_remote_agent(
    task_id: &str,
    server_url: &str,
    prompt: &str,
    model: &str,
    tasks_dir: &Path,
) -> OxiResult<TaskStatus> {
    tracing::info!(
        "run_remote_agent task={} server={}",
        task_id,
        server_url
    );

    let mut out_file = open_output_file(tasks_dir, task_id)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| OxiError::Other(format!("http client build: {e}")))?;

    let body = json!({
        "prompt": prompt,
        "model": model,
    });

    // Retry loop with exponential backoff.
    let mut last_error = String::new();
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let delay = Duration::from_millis(BASE_RETRY_DELAY_MS * 2u64.pow(attempt - 1));
            tracing::warn!(
                "task={} retry {}/{} after {:?}",
                task_id,
                attempt + 1,
                MAX_RETRIES,
                delay
            );
            write_line(
                &mut out_file,
                "stderr",
                &format!("retrying ({}/{})", attempt + 1, MAX_RETRIES),
            )?;
            tokio::time::sleep(delay).await;
        }

        let url = format!("{}/v1/prompt", server_url.trim_end_matches('/'));
        match client.post(&url).json(&body).send().await {
            Ok(response) => {
                let status_code = response.status();
                if !status_code.is_success() {
                    let err_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "unknown".to_string());
                    last_error = format!("HTTP {status_code}: {err_text}");
                    write_line(&mut out_file, "stderr", &last_error)?;

                    // Only retry on server errors, not client errors.
                    if status_code.is_server_error() {
                        continue;
                    }
                    return Ok(TaskStatus::Failed { error: last_error });
                }

                // Stream the response body line by line.
                let response_text = response
                    .text()
                    .await
                    .map_err(|e| OxiError::Other(format!("response read: {e}")))?;

                for line in response_text.lines() {
                    if !line.trim().is_empty() {
                        write_line(&mut out_file, "stdout", line)?;
                    }
                }

                tracing::info!("run_remote_agent task={} completed", task_id);
                return Ok(TaskStatus::Completed { exit_code: 0 });
            }
            Err(e) => {
                last_error = format!("connection error: {e}");
                write_line(&mut out_file, "stderr", &last_error)?;
                tracing::warn!("task={} attempt {} failed: {}", task_id, attempt + 1, e);
            }
        }
    }

    Ok(TaskStatus::Failed {
        error: format!("all {MAX_RETRIES} attempts failed: {last_error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn tmp_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("oxi-remote-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn run_remote_agent_connection_refused() {
        let dir = tmp_dir();
        // Connect to a port that should refuse connections.
        let status = run_remote_agent(
            "t-remote-1",
            "http://127.0.0.1:19999",
            "hello",
            "test-model",
            &dir,
        )
        .await
        .unwrap();
        assert!(
            matches!(status, TaskStatus::Failed { .. }),
            "expected failure on unreachable server"
        );
    }

    #[test]
    fn output_file_creation() {
        let dir = tmp_dir();
        let mut f = open_output_file(&dir, "remote-test").unwrap();
        write_line(&mut f, "stdout", "hello remote").unwrap();
        let content = std::fs::read_to_string(dir.join("remote-test/output.jsonl")).unwrap();
        assert!(content.contains("hello remote"));
    }
}
