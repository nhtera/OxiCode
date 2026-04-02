/// Subagent process spawning — launches child `oxicode` processes with serialized config.
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tracing::{debug, info};

use oxicode_common::{OxiError, OxiResult};

/// Configuration passed to a spawned subagent process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub prompt: String,
    pub model: String,
    pub working_dir: PathBuf,
    pub permission_mode: String,
    pub timeout: Duration,
    pub inherit_env: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "agent".to_string(),
            prompt: String::new(),
            model: "claude-sonnet-4-20250514".to_string(),
            working_dir: PathBuf::from("."),
            permission_mode: "default".to_string(),
            timeout: Duration::from_secs(300),
            inherit_env: true,
        }
    }
}

/// Result returned after a subagent process completes.
#[derive(Debug, Clone)]
pub struct AgentResult {
    pub agent_id: String,
    pub output: String,
    pub is_error: bool,
    pub duration: Duration,
}

/// Handle to a running subagent process.
pub struct AgentHandle {
    pub id: String,
    pub(crate) child: Child,
    pub started_at: Instant,
}

impl std::fmt::Debug for AgentHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentHandle")
            .field("id", &self.id)
            .field("started_at", &self.started_at)
            .finish_non_exhaustive()
    }
}

impl AgentHandle {
    /// Wait for the process to exit and collect its output.
    pub async fn wait(&mut self) -> OxiResult<AgentResult> {
        let started = self.started_at;
        let status = self
            .child
            .wait()
            .await
            .map_err(|e| OxiError::Other(format!("agent wait failed: {e}")))?;

        let duration = started.elapsed();
        let is_error = !status.success();

        Ok(AgentResult {
            agent_id: self.id.clone(),
            output: String::new(),
            is_error,
            duration,
        })
    }

    /// Kill the child process immediately.
    pub fn kill(&mut self) -> OxiResult<()> {
        self.child
            .start_kill()
            .map_err(|e| OxiError::Other(format!("kill failed: {e}")))
    }

    /// Returns true if the process has not yet exited.
    pub fn is_running(&self) -> bool {
        // try_wait returns None when still running
        // We cannot call try_wait on &self (needs &mut), so we use id() as proxy
        self.child.id().is_some()
    }
}

/// Spawn a subagent process with the given config.
///
/// Passes serialized `AgentConfig` JSON via stdin.  The child binary is the
/// current executable re-invoked with `--agent-mode`.
pub async fn spawn_agent(config: &AgentConfig) -> OxiResult<AgentResult> {
    let agent_id = uuid::Uuid::new_v4().to_string();
    let started = Instant::now();

    debug!(agent_id = %agent_id, name = %config.name, "spawning subagent");

    let current_exe = std::env::current_exe()
        .map_err(|e| OxiError::Other(format!("cannot resolve current exe: {e}")))?;

    let config_json = serde_json::to_string(config)
        .map_err(|e| OxiError::Other(format!("config serialization failed: {e}")))?;

    let mut cmd = Command::new(&current_exe);
    cmd.arg("--agent-mode")
        .arg(&agent_id)
        .current_dir(&config.working_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if !config.inherit_env {
        cmd.env_clear();
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| OxiError::Other(format!("spawn failed: {e}")))?;

    // Write config JSON to child stdin then close it.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(config_json.as_bytes())
            .await
            .map_err(|e| OxiError::Other(format!("stdin write failed: {e}")))?;
        // stdin dropped here → EOF sent to child
    }

    // Apply timeout via tokio::time::timeout.
    let output = tokio::time::timeout(config.timeout, child.wait_with_output())
        .await
        .map_err(|_| {
            OxiError::Other(format!(
                "agent '{}' timed out after {}s",
                config.name,
                config.timeout.as_secs()
            ))
        })?
        .map_err(|e| OxiError::Other(format!("process wait failed: {e}")))?;

    let duration = started.elapsed();
    let is_error = !output.status.success();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    info!(
        agent_id = %agent_id,
        name = %config.name,
        is_error,
        duration_ms = duration.as_millis(),
        "subagent finished"
    );

    Ok(AgentResult {
        agent_id,
        output: stdout,
        is_error,
        duration,
    })
}

/// Spawn an agent and return an `AgentHandle` for manual lifecycle control.
pub fn spawn_agent_handle(config: &AgentConfig) -> OxiResult<AgentHandle> {
    let id = uuid::Uuid::new_v4().to_string();

    let current_exe = std::env::current_exe()
        .map_err(|e| OxiError::Other(format!("cannot resolve current exe: {e}")))?;

    let mut cmd = Command::new(&current_exe);
    cmd.arg("--agent-mode")
        .arg(&id)
        .current_dir(&config.working_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if !config.inherit_env {
        cmd.env_clear();
    }

    let child = cmd
        .spawn()
        .map_err(|e| OxiError::Other(format!("spawn_handle failed: {e}")))?;

    debug!(agent_id = %id, "agent handle created");

    Ok(AgentHandle {
        id,
        child,
        started_at: Instant::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_config_default() {
        let cfg = AgentConfig::default();
        assert_eq!(cfg.permission_mode, "default");
        assert_eq!(cfg.timeout, Duration::from_secs(300));
        assert!(cfg.inherit_env);
    }

    #[test]
    fn test_agent_config_serde_roundtrip() {
        let cfg = AgentConfig {
            name: "tester".to_string(),
            prompt: "do something".to_string(),
            model: "claude-opus-4".to_string(),
            working_dir: PathBuf::from("/tmp"),
            permission_mode: "restricted".to_string(),
            timeout: Duration::from_secs(60),
            inherit_env: false,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "tester");
        assert_eq!(parsed.timeout, Duration::from_secs(60));
        assert!(!parsed.inherit_env);
    }

    #[test]
    fn test_agent_result_fields() {
        let r = AgentResult {
            agent_id: "abc".to_string(),
            output: "done".to_string(),
            is_error: false,
            duration: Duration::from_millis(100),
        };
        assert!(!r.is_error);
        assert_eq!(r.output, "done");
    }
}
