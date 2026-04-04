//! Hook configuration: load hook definitions from settings.
//!
//! Hooks are defined in settings.toml under `[hooks]`:
//! ```toml
//! [hooks]
//! session_start = "~/.oxicode/hooks/session-start.sh"
//! pre_query = "~/.oxicode/hooks/pre-query.sh"
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::events::HookEvent;

/// A single hook definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDef {
    /// Shell command to execute.
    pub command: String,
    /// Timeout in seconds (default 10).
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Whether this hook is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_timeout() -> u64 {
    10
}

fn default_enabled() -> bool {
    true
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
                // Simple string form: `session_start = "command"`
                toml::Value::String(cmd) => HookDef {
                    command: cmd.clone(),
                    timeout_secs: default_timeout(),
                    enabled: true,
                },
                // Table form: `[hooks.session_start] command = "..." timeout_secs = 5`
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_string_form() {
        let toml_str = r#"
session_start = "echo hello"
pre_query = "~/.oxicode/hooks/pre-query.sh"
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert_eq!(config.hooks.len(), 2);
        assert_eq!(
            config.get(HookEvent::SessionStart).unwrap().command,
            "echo hello"
        );
    }

    #[test]
    fn test_table_form() {
        let toml_str = r#"
[pre_query]
command = "python3 check.py"
timeout_secs = 5
enabled = true
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let hook = config.get(HookEvent::PreQuery).unwrap();
        assert_eq!(hook.command, "python3 check.py");
        assert_eq!(hook.timeout_secs, 5);
    }

    #[test]
    fn test_disabled_hook() {
        let toml_str = r#"
[session_start]
command = "echo disabled"
enabled = false
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert!(config.get(HookEvent::SessionStart).is_none());
    }

    // -- New comprehensive tests --

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
        let toml_str = r#"session_start = "echo hello""#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let hook = config.get(HookEvent::SessionStart).unwrap();
        assert_eq!(hook.timeout_secs, 10);
    }

    #[test]
    fn test_custom_timeout() {
        let toml_str = r#"
[tool_call_before]
command = "security-check.sh"
timeout_secs = 30
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let hook = config.get(HookEvent::ToolCallBefore).unwrap();
        assert_eq!(hook.timeout_secs, 30);
    }

    #[test]
    fn test_table_default_enabled() {
        let toml_str = r#"
[error]
command = "log-error.sh"
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let hook = config.get(HookEvent::Error).unwrap();
        assert!(hook.enabled);
    }

    #[test]
    fn test_multiple_hooks() {
        let toml_str = r#"
session_start = "init.sh"
session_end = "cleanup.sh"
pre_query = "pre.sh"
post_sampling = "post.sh"
tool_call_before = "check.sh"
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert_eq!(config.hooks.len(), 5);
        assert!(config.get(HookEvent::SessionStart).is_some());
        assert!(config.get(HookEvent::SessionEnd).is_some());
        assert!(config.get(HookEvent::PreQuery).is_some());
        assert!(config.get(HookEvent::PostSampling).is_some());
        assert!(config.get(HookEvent::ToolCallBefore).is_some());
    }

    #[test]
    fn test_unknown_event_key_stored_but_not_retrievable() {
        let toml_str = r#"unknown_event = "noop.sh""#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert_eq!(config.hooks.len(), 1);
        // Can't retrieve it via HookEvent since "unknown_event" doesn't match any event.
        // All known events should return None.
        assert!(config.get(HookEvent::SessionStart).is_none());
    }

    #[test]
    fn test_non_string_non_table_value_skipped() {
        let toml_str = r#"session_start = 42"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert!(config.hooks.is_empty());
    }

    #[test]
    fn test_hook_def_serde_roundtrip() {
        let def = HookDef {
            command: "echo test".to_string(),
            timeout_secs: 15,
            enabled: true,
        };
        let json = serde_json::to_string(&def).unwrap();
        let parsed: HookDef = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.command, "echo test");
        assert_eq!(parsed.timeout_secs, 15);
        assert!(parsed.enabled);
    }

    #[test]
    fn test_all_29_events_retrievable() {
        // Build a config with all 29 events configured.
        let mut hooks = HashMap::new();
        for event in HookEvent::ALL {
            hooks.insert(
                event.as_str().to_string(),
                HookDef {
                    command: format!("hook-{}.sh", event.as_str()),
                    timeout_secs: 10,
                    enabled: true,
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
}
