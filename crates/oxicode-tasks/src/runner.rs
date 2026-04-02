use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use oxicode_common::{OxiError, OxiResult};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::manager::TaskStatus;

const STALL_TIMEOUT_SECS: u64 = 45;

/// Open (and optionally create) the output JSONL file for a task.
fn open_output_file(tasks_dir: &Path, task_id: &str) -> OxiResult<std::fs::File> {
    let dir = tasks_dir.join(task_id);
    std::fs::create_dir_all(&dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("output.jsonl"))?;
    Ok(file)
}

/// Append one JSON line to the output file.
fn write_line(file: &mut std::fs::File, stream: &str, line: &str) -> OxiResult<()> {
    let record = json!({
        "ts": Utc::now().to_rfc3339(),
        "stream": stream,
        "line": line,
    });
    writeln!(file, "{record}")?;
    Ok(())
}

/// Spawn `sh -c <command>`, stream output to disk, detect stalls, return final status.
pub async fn run_bash(task_id: &str, command: &str, tasks_dir: &Path) -> OxiResult<TaskStatus> {
    tracing::info!("run_bash task={} cmd={:?}", task_id, command);

    let mut child = Command::new("sh")
        .args(["-c", command])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| OxiError::Other(format!("spawn failed: {e}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| OxiError::Other("no stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| OxiError::Other("no stderr".into()))?;

    let mut out_file = open_output_file(tasks_dir, task_id)?;
    let mut err_file = open_output_file(tasks_dir, task_id)
        .map_err(|e| OxiError::Other(format!("output file (stderr dup): {e}")))?;

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();
    let stall = Duration::from_secs(STALL_TIMEOUT_SECS);
    let mut stdout_done = false;
    let mut stderr_done = false;

    while !stdout_done || !stderr_done {
        tokio::select! {
            res = tokio::time::timeout(stall, stdout_reader.next_line()), if !stdout_done => {
                match res {
                    Err(_) => {
                        let _ = child.kill().await;
                        tracing::warn!("task={} stalled after {}s", task_id, STALL_TIMEOUT_SECS);
                        return Ok(TaskStatus::Failed {
                            error: format!("no output for {STALL_TIMEOUT_SECS}s"),
                        });
                    }
                    Ok(Ok(Some(line))) => {
                        write_line(&mut out_file, "stdout", &line)?;
                    }
                    Ok(Ok(None)) => stdout_done = true,
                    Ok(Err(e)) => {
                        return Err(OxiError::Other(format!("stdout read error: {e}")));
                    }
                }
            }
            res = tokio::time::timeout(stall, stderr_reader.next_line()), if !stderr_done => {
                match res {
                    Err(_) => {
                        let _ = child.kill().await;
                        tracing::warn!("task={} stalled (stderr) after {}s", task_id, STALL_TIMEOUT_SECS);
                        return Ok(TaskStatus::Failed {
                            error: format!("no output for {STALL_TIMEOUT_SECS}s"),
                        });
                    }
                    Ok(Ok(Some(line))) => {
                        write_line(&mut err_file, "stderr", &line)?;
                    }
                    Ok(Ok(None)) => stderr_done = true,
                    Ok(Err(e)) => {
                        return Err(OxiError::Other(format!("stderr read error: {e}")));
                    }
                }
            }
        }
    }

    let exit_status = child
        .wait()
        .await
        .map_err(|e| OxiError::Other(format!("wait failed: {e}")))?;

    let exit_code = exit_status.code().unwrap_or(-1);
    tracing::info!("task={} finished exit_code={}", task_id, exit_code);

    if exit_code == 0 {
        Ok(TaskStatus::Completed { exit_code })
    } else {
        Ok(TaskStatus::Failed {
            error: format!("exit code {exit_code}"),
        })
    }
}

/// Spawn this binary in `--task-mode`, pipe prompt via stdin, capture output.
pub async fn run_agent(
    task_id: &str,
    prompt: &str,
    model: &str,
    tasks_dir: &Path,
) -> OxiResult<TaskStatus> {
    tracing::info!("run_agent task={} model={}", task_id, model);

    let exe = std::env::current_exe().map_err(|e| OxiError::Other(format!("current_exe: {e}")))?;

    let mut child = Command::new(&exe)
        .args(["--task-mode", "--model", model])
        .env("OXICODE_TASK_ID", task_id)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| OxiError::Other(format!("agent spawn failed: {e}")))?;

    // Write prompt to stdin then close it.
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt as _;
        stdin
            .write_all(prompt.as_bytes())
            .await
            .map_err(|e| OxiError::Other(format!("stdin write: {e}")))?;
    }

    // Reuse bash output streaming.
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| OxiError::Other("no stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| OxiError::Other("no stderr".into()))?;

    let mut out_file = open_output_file(tasks_dir, task_id)?;
    let mut err_file = open_output_file(tasks_dir, task_id)?;

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();
    let stall = Duration::from_secs(STALL_TIMEOUT_SECS);
    let mut stdout_done = false;
    let mut stderr_done = false;

    while !stdout_done || !stderr_done {
        tokio::select! {
            res = tokio::time::timeout(stall, stdout_reader.next_line()), if !stdout_done => {
                match res {
                    Err(_) => {
                        let _ = child.kill().await;
                        return Ok(TaskStatus::Failed {
                            error: format!("agent stalled after {STALL_TIMEOUT_SECS}s"),
                        });
                    }
                    Ok(Ok(Some(line))) => write_line(&mut out_file, "stdout", &line)?,
                    Ok(Ok(None)) => stdout_done = true,
                    Ok(Err(e)) => return Err(OxiError::Other(format!("stdout: {e}"))),
                }
            }
            res = tokio::time::timeout(stall, stderr_reader.next_line()), if !stderr_done => {
                match res {
                    Err(_) => {
                        let _ = child.kill().await;
                        return Ok(TaskStatus::Failed {
                            error: format!("agent stalled after {STALL_TIMEOUT_SECS}s"),
                        });
                    }
                    Ok(Ok(Some(line))) => write_line(&mut err_file, "stderr", &line)?,
                    Ok(Ok(None)) => stderr_done = true,
                    Ok(Err(e)) => return Err(OxiError::Other(format!("stderr: {e}"))),
                }
            }
        }
    }

    let exit_status = child
        .wait()
        .await
        .map_err(|e| OxiError::Other(format!("agent wait: {e}")))?;

    let exit_code = exit_status.code().unwrap_or(-1);
    tracing::info!("run_agent task={} exit_code={}", task_id, exit_code);

    if exit_code == 0 {
        Ok(TaskStatus::Completed { exit_code })
    } else {
        Ok(TaskStatus::Failed {
            error: format!("agent exit code {exit_code}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn tmp_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("oxi-runner-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn run_bash_success() {
        let dir = tmp_dir();
        let status = run_bash("t1", "echo hello", &dir).await.unwrap();
        assert!(matches!(status, TaskStatus::Completed { exit_code: 0 }));
        let output = std::fs::read_to_string(dir.join("t1/output.jsonl")).unwrap();
        assert!(output.contains("hello"));
    }

    #[tokio::test]
    async fn run_bash_nonzero_exit_is_failed() {
        let dir = tmp_dir();
        let status = run_bash("t2", "exit 42", &dir).await.unwrap();
        assert!(matches!(status, TaskStatus::Failed { .. }));
    }
}
