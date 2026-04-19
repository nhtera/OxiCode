//! Hook configuration: load hook definitions from settings.
//!
//! Hooks are defined in settings.toml under `[hooks]`:
//! ```toml
//! [hooks]
//! # Shell command (default, backward compatible)
//! session_start = "~/.oxicode/hooks/session-start.sh"
//!
//! # Explicit command type
//! [hooks.pre_query]
//! type = "command"
//! command = "~/.oxicode/hooks/pre-query.sh"
//!
//! # Agent type (LLM call)
//! [hooks.tool_call_before]
//! type = "agent"
//! instructions = "Check for PII in tool arguments"
//! model = "claude-haiku"
//! timeout_secs = 60
//!
//! # HTTP type (POST to URL)
//! [hooks.post_sampling]
//! type = "http"
//! url = "https://hooks.example.com/post-sampling"
//! authorization = "Bearer token123"
//! timeout_secs = 30
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::agent_hook_executor::AgentHookConfig;
use crate::events::HookEvent;
use crate::http_hook_executor::HttpHookConfig;

/// Hook execution type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookType {
    /// Shell subprocess (default, backward compatible).
    #[default]
    Command,
    /// LLM agent call with structured output.
    Agent,
    /// HTTP POST to a URL endpoint.
    Http,
}

/// A single hook definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDef {
    /// Hook execution type (default: Command).
    #[serde(default, rename = "type")]
    pub hook_type: HookType,
    /// Shell command to execute (for Command type).
    #[serde(default)]
    pub command: String,
    /// Timeout in seconds (default 10 for command, 60 for agent, 600 for http).
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Whether this hook is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Agent-specific config (only used when hook_type = Agent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentHookConfig>,
    /// HTTP-specific config (only used when hook_type = Http).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpHookConfig>,

    // -- Inline agent fields (flat config alternative) --
    /// Agent instructions (shorthand: set type="agent" + instructions directly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Agent model override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    // -- Inline HTTP fields (flat config alternative) --
    /// HTTP URL (shorthand: set type="http" + url directly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// HTTP authorization header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,
}

fn default_timeout() -> u64 {
    10
}

fn default_enabled() -> bool {
    true
}

impl HookDef {
    /// Build an `AgentHookConfig` from this definition (merging inline fields).
    pub fn agent_config(&self) -> AgentHookConfig {
        if let Some(ref cfg) = self.agent {
            return cfg.clone();
        }
        AgentHookConfig {
            instructions: self.instructions.clone().unwrap_or_default(),
            model: self
                .model
                .clone()
                .unwrap_or_else(|| "claude-haiku".to_string()),
            timeout_secs: self.timeout_secs,
            max_tokens: 256,
        }
    }

    /// Build an `HttpHookConfig` from this definition (merging inline fields).
    pub fn http_config(&self) -> HttpHookConfig {
        if let Some(ref cfg) = self.http {
            return cfg.clone();
        }
        HttpHookConfig {
            url: self.url.clone().unwrap_or_default(),
            authorization: self.authorization.clone(),
            env_headers: std::collections::HashMap::new(),
            timeout_secs: self.timeout_secs,
        }
    }
}

/// All hook configurations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksConfig {
    #[serde(flatten)]
    pub hooks: HashMap<String, HookDef>,
}

impl HooksConfig {
    /// Load hooks from a TOML table value (the `[hooks]` section of settings).
    pub fn from_toml_value(value: &toml::Value) -> Self {
        let Some(table) = value.as_table() else {
            return Self::default();
        };

        let mut hooks = HashMap::new();
        for (key, val) in table {
            let def = match val {
                // Simple string form: `session_start = "command"` → Command type.
                toml::Value::String(cmd) => HookDef {
                    hook_type: HookType::Command,
                    command: cmd.clone(),
                    timeout_secs: default_timeout(),
                    enabled: true,
                    agent: None,
                    http: None,
                    instructions: None,
                    model: None,
                    url: None,
                    authorization: None,
                },
                // Table form: parse full HookDef with type detection.
                toml::Value::Table(_) => {
                    match toml::from_str::<HookDef>(&toml::to_string(val).unwrap_or_default()) {
                        Ok(def) => def,
                        Err(e) => {
                            tracing::warn!("Invalid hook config for '{key}': {e}");
                            continue;
                        }
                    }
                }
                _ => continue,
            };
            hooks.insert(key.clone(), def);
        }

        Self { hooks }
    }

    /// Get the hook definition for a specific event, if configured and enabled.
    pub fn get(&self, event: HookEvent) -> Option<&HookDef> {
        self.hooks.get(event.as_str()).filter(|def| def.enabled)
    }

    /// Load hooks config from the user settings file.
    pub fn load_from_settings_dir() -> Self {
        let config_dir =
            dirs::home_dir().map_or_else(|| PathBuf::from(".oxicode"), |h| h.join(".oxicode"));

        let settings_path = config_dir.join("settings.toml");
        let Ok(content) = std::fs::read_to_string(&settings_path) else {
            return Self::default();
        };

        let parsed: toml::Value = match content.parse() {
            Ok(v) => v,
            Err(_) => return Self::default(),
        };

        parsed
            .get("hooks")
            .map(Self::from_toml_value)
            .unwrap_or_default()
    }

    /// Layered loader (Claude Code parity, oxicode-overlay-wins precedence).
    ///
    /// Read order (low → high precedence):
    /// 1. `~/.claude/settings.json` (user)
    /// 2. project `.claude/settings.json`
    /// 3. project `.claude/settings.local.json`
    /// 4. `~/.oxicode/settings.toml` overlay (oxicode wins per user choice)
    ///
    /// Each layer's hooks are merged into a single `HooksConfig`. Later
    /// layers' hooks for the same event REPLACE earlier ones.
    pub fn load_layered() -> Self {
        let mut merged = Self::default();

        // Layer 1: ~/.claude/settings.json
        if let Some(home) = dirs::home_dir() {
            let user_claude = home.join(".claude").join("settings.json");
            merge_into(&mut merged, &load_claude_json(&user_claude));
        }

        // Layer 2: project .claude/settings.json (relative to cwd)
        let project_claude = std::path::PathBuf::from(".claude").join("settings.json");
        merge_into(&mut merged, &load_claude_json(&project_claude));

        // Layer 3: project .claude/settings.local.json
        let project_local = std::path::PathBuf::from(".claude").join("settings.local.json");
        merge_into(&mut merged, &load_claude_json(&project_local));

        // Layer 4: ~/.oxicode/settings.toml overlay (highest precedence).
        merge_into(&mut merged, &Self::load_from_settings_dir());

        merged
    }
}

/// Merge `src` into `dst`, with `src` entries replacing `dst` entries on conflict.
fn merge_into(dst: &mut HooksConfig, src: &HooksConfig) {
    for (key, def) in &src.hooks {
        dst.hooks.insert(key.clone(), def.clone());
    }
}

/// Read a Claude-Code-style `settings.json` file and convert its `hooks`
/// section to our `HooksConfig`. Returns empty config if the file is missing
/// or malformed (fail-soft — hooks should never break the app).
fn load_claude_json(path: &std::path::Path) -> HooksConfig {
    let Ok(content) = std::fs::read_to_string(path) else {
        return HooksConfig::default();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
        tracing::warn!("Failed to parse Claude settings JSON at {path:?}");
        return HooksConfig::default();
    };
    let Some(hooks_section) = parsed.get("hooks") else {
        return HooksConfig::default();
    };
    HooksConfig::from_claude_json(hooks_section)
}

impl HooksConfig {
    /// Parse Claude Code's `hooks` JSON section.
    ///
    /// Schema:
    /// ```json
    /// {
    ///   "PreToolUse": [
    ///     { "matcher": "Bash|Edit",
    ///       "hooks": [{"type":"command","command":"...","timeout":30}] }
    ///   ]
    /// }
    /// ```
    /// We currently flatten to one hook per event (first matching). Multi-hook
    /// + matcher routing is a follow-up phase.
    pub fn from_claude_json(value: &serde_json::Value) -> Self {
        let Some(obj) = value.as_object() else {
            return Self::default();
        };
        let mut hooks = HashMap::new();
        for (event_name, event_value) in obj {
            // event_value is an array of {matcher, hooks: [...]}
            let Some(arr) = event_value.as_array() else {
                continue;
            };
            // Take the first matcher group's first hook (single-hook MVP).
            for matcher_group in arr {
                let Some(group) = matcher_group.as_object() else {
                    continue;
                };
                let Some(hooks_arr) = group.get("hooks").and_then(|v| v.as_array()) else {
                    continue;
                };
                if let Some(first) = hooks_arr.first() {
                    if let Ok(def) = serde_json::from_value::<HookDef>(first.clone()) {
                        hooks.insert(event_name.clone(), def);
                        break;
                    }
                }
            }
        }
        Self { hooks }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_string_form_pascal_case() {
        let toml_str = r#"
SessionStart = "echo hello"
UserPromptSubmit = "~/.claude/hooks/pre-prompt.sh"
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert_eq!(config.hooks.len(), 2);
        let hook = config.get(HookEvent::SessionStart).unwrap();
        assert_eq!(hook.command, "echo hello");
        assert_eq!(hook.hook_type, HookType::Command);
    }

    #[test]
    fn test_table_form_command() {
        let toml_str = r#"
[UserPromptSubmit]
command = "python3 check.py"
timeout_secs = 5
enabled = true
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let hook = config.get(HookEvent::UserPromptSubmit).unwrap();
        assert_eq!(hook.command, "python3 check.py");
        assert_eq!(hook.timeout_secs, 5);
        assert_eq!(hook.hook_type, HookType::Command);
    }

    #[test]
    fn test_table_form_agent() {
        let toml_str = r#"
[PreToolUse]
type = "agent"
instructions = "Check for PII"
model = "claude-haiku"
timeout_secs = 30
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let hook = config.get(HookEvent::PreToolUse).unwrap();
        assert_eq!(hook.hook_type, HookType::Agent);
        let agent_cfg = hook.agent_config();
        assert_eq!(agent_cfg.instructions, "Check for PII");
        assert_eq!(agent_cfg.model, "claude-haiku");
        assert_eq!(agent_cfg.timeout_secs, 30);
    }

    #[test]
    fn test_table_form_http() {
        let toml_str = r#"
[Stop]
type = "http"
url = "https://hooks.example.com/event"
authorization = "Bearer abc"
timeout_secs = 120
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let hook = config.get(HookEvent::Stop).unwrap();
        assert_eq!(hook.hook_type, HookType::Http);
        let http_cfg = hook.http_config();
        assert_eq!(http_cfg.url, "https://hooks.example.com/event");
        assert_eq!(http_cfg.authorization.as_deref(), Some("Bearer abc"));
        assert_eq!(http_cfg.timeout_secs, 120);
    }

    #[test]
    fn test_disabled_hook() {
        let toml_str = r#"
[SessionStart]
command = "echo disabled"
enabled = false
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert!(config.get(HookEvent::SessionStart).is_none());
    }

    #[test]
    fn test_hook_type_default_is_command() {
        assert_eq!(HookType::default(), HookType::Command);
    }

    #[test]
    fn test_empty_config() {
        let config = HooksConfig::default();
        assert!(config.hooks.is_empty());
        assert!(config.get(HookEvent::SessionStart).is_none());
    }

    #[test]
    fn test_from_non_table_value() {
        let value: toml::Value = toml::Value::String("not a table".to_string());
        let config = HooksConfig::from_toml_value(&value);
        assert!(config.hooks.is_empty());
    }

    #[test]
    fn test_default_timeout() {
        let toml_str = r#"SessionStart = "echo hello""#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let hook = config.get(HookEvent::SessionStart).unwrap();
        assert_eq!(hook.timeout_secs, 10);
    }

    #[test]
    fn test_unknown_event_key_stored_but_not_retrievable() {
        let toml_str = r#"NotARealEvent = "noop.sh""#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert_eq!(config.hooks.len(), 1);
        assert!(config.get(HookEvent::SessionStart).is_none());
    }

    #[test]
    fn test_old_snake_case_keys_do_not_match_new_events() {
        // Hard rename: old snake_case keys must not be picked up.
        let toml_str = r#"
tool_call_before = "old.sh"
pre_query = "old.sh"
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert_eq!(config.hooks.len(), 2, "raw keys still stored");
        assert!(config.get(HookEvent::PreToolUse).is_none());
        assert!(config.get(HookEvent::UserPromptSubmit).is_none());
    }

    #[test]
    fn test_all_events_retrievable() {
        let mut hooks = HashMap::new();
        for event in HookEvent::ALL {
            hooks.insert(
                event.as_str().to_string(),
                HookDef {
                    hook_type: HookType::Command,
                    command: format!("hook-{}.sh", event.as_str()),
                    timeout_secs: 10,
                    enabled: true,
                    agent: None,
                    http: None,
                    instructions: None,
                    model: None,
                    url: None,
                    authorization: None,
                },
            );
        }
        let config = HooksConfig { hooks };
        for event in HookEvent::ALL {
            let hook = config.get(*event);
            assert!(hook.is_some(), "Event {:?} should be retrievable", event);
            assert!(hook.unwrap().command.contains(event.as_str()));
        }
    }

    #[test]
    fn test_from_claude_json_simple() {
        let json = serde_json::json!({
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [
                        {"type": "command", "command": "echo pre", "timeout_secs": 5}
                    ]
                }
            ],
            "SessionStart": [
                {
                    "hooks": [{"type": "command", "command": "init.sh"}]
                }
            ]
        });
        let config = HooksConfig::from_claude_json(&json);
        let pre = config.get(HookEvent::PreToolUse).unwrap();
        assert_eq!(pre.command, "echo pre");
        let start = config.get(HookEvent::SessionStart).unwrap();
        assert_eq!(start.command, "init.sh");
    }

    #[test]
    fn test_from_claude_json_ignores_malformed_entries() {
        let json = serde_json::json!({
            "PreToolUse": "not an array",
        });
        let config = HooksConfig::from_claude_json(&json);
        assert!(config.hooks.is_empty());
    }
}
