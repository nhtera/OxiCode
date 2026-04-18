/// Subagent process spawning — launches child `oxicode` processes with serialized config.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tracing::{debug, info};

use oxicode_common::{OxiError, OxiResult};
use oxicode_hooks::{HookEvent, HookManager};

use crate::built_in::AgentType;

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
    /// Optional specialized agent type — restricts tools and injects a tailored prompt.
    #[serde(default)]
    pub agent_type: Option<AgentType>,
    /// Optional tool whitelist — when set, only these tools are available to the subagent.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    /// True when the caller explicitly set the model (prevents agent type from overriding it).
    #[serde(default)]
    pub model_override: bool,
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
            agent_type: None,
            allowed_tools: None,
            model_override: false,
        }
    }
}

impl AgentConfig {
    /// Apply agent type defaults: inject system prompt prefix, set model, populate tool whitelist.
    ///
    /// Model is only overridden when `model_override` is false (caller didn't explicitly set one).
    pub fn apply_agent_type(&mut self) {
        if let Some(at) = self.agent_type {
            // Prepend the type-specific system prompt to the user's task prompt.
            let sys = at.system_prompt();
            self.prompt = format!("{sys}\n\n---\n\n{}", self.prompt);

            // Use the type's default model unless caller explicitly overrode it.
            if !self.model_override {
                self.model = at.default_model().to_string();
            }

            // Set tool whitelist from the agent type (General = no restriction).
            if self.allowed_tools.is_none() {
                self.allowed_tools = at
                    .allowed_tools()
                    .map(|t| t.iter().map(|s| (*s).to_string()).collect());
            }
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
/// Fires `AgentSpawn` before and `AgentComplete` after if `hook_manager` is provided.
pub async fn spawn_agent(
    config: &AgentConfig,
    hook_manager: Option<&Arc<HookManager>>,
) -> OxiResult<AgentResult> {
    let agent_id = uuid::Uuid::new_v4().to_string();
    let started = Instant::now();

    debug!(agent_id = %agent_id, name = %config.name, "spawning subagent");

    // Fire AgentSpawn hook.
    if let Some(hm) = hook_manager {
        let data = serde_json::json!({
            "agent_id": agent_id,
            "name": config.name,
            "model": config.model,
            "agent_type": format!("{:?}", config.agent_type),
        });
        hm.fire(HookEvent::AgentSpawn, data).await;
    }

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

    // Fire AgentComplete hook.
    if let Some(hm) = hook_manager {
        let data = serde_json::json!({
            "agent_id": agent_id,
            "name": config.name,
            "is_error": is_error,
            "duration_ms": duration.as_millis() as u64,
        });
        hm.fire(HookEvent::AgentComplete, data).await;
    }

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
        assert!(cfg.agent_type.is_none());
        assert!(cfg.allowed_tools.is_none());
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
            agent_type: Some(AgentType::Explore),
            allowed_tools: None,
            model_override: true,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "tester");
        assert_eq!(parsed.timeout, Duration::from_secs(60));
        assert!(!parsed.inherit_env);
        assert_eq!(parsed.agent_type, Some(AgentType::Explore));
    }

    #[test]
    fn test_apply_agent_type_sets_tools_and_prompt() {
        let mut cfg = AgentConfig {
            prompt: "find all TODO comments".to_string(),
            agent_type: Some(AgentType::Explore),
            ..Default::default()
        };
        cfg.apply_agent_type();

        // System prompt is prepended.
        assert!(cfg.prompt.contains("exploration agent"));
        assert!(cfg.prompt.contains("find all TODO comments"));

        // Model updated to Explore's default (haiku).
        assert!(cfg.model.contains("haiku"));

        // Tool whitelist populated.
        let tools = cfg.allowed_tools.unwrap();
        assert!(tools.contains(&"glob".to_string()));
        assert!(tools.contains(&"grep".to_string()));
        assert!(!tools.contains(&"file_write".to_string()));
    }

    #[test]
    fn test_apply_agent_type_general_no_restrictions() {
        let mut cfg = AgentConfig {
            prompt: "do anything".to_string(),
            agent_type: Some(AgentType::General),
            ..Default::default()
        };
        cfg.apply_agent_type();
        assert!(cfg.allowed_tools.is_none());
    }

    #[test]
    fn test_apply_agent_type_preserves_custom_model() {
        let mut cfg = AgentConfig {
            prompt: "plan something".to_string(),
            model: "claude-opus-4".to_string(),
            model_override: true,
            agent_type: Some(AgentType::Plan),
            ..Default::default()
        };
        cfg.apply_agent_type();
        // Custom model should be preserved (not overridden by type default).
        assert_eq!(cfg.model, "claude-opus-4");
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
