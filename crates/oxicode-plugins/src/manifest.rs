//! Plugin manifest parser (plugin.toml).
//!
//! Each plugin ships a `plugin.toml` declaring its name, version, tools, commands,
//! hooks, and lifecycle scripts. This module parses and validates that manifest.

use std::collections::HashMap;
use std::path::Path;

use oxicode_common::{OxiError, OxiResult};
use serde::{Deserialize, Serialize};

/// Top-level plugin manifest parsed from `plugin.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,

    /// The command to spawn the plugin subprocess.
    pub command: String,
    /// Arguments passed to the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables for the subprocess.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Tools this plugin provides.
    #[serde(default)]
    pub tools: Vec<PluginToolDef>,
    /// Slash commands this plugin provides.
    #[serde(default)]
    pub commands: Vec<PluginCommandDef>,
    /// Hook events this plugin subscribes to.
    #[serde(default)]
    pub hooks: Vec<PluginHookDef>,
    /// Lifecycle scripts (init, shutdown).
    #[serde(default)]
    pub lifecycle: PluginLifecycle,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

/// A tool provided by the plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginToolDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema for the tool's input (as TOML inline table or string).
    #[serde(default)]
    pub input_schema: serde_json::Value,
}

/// A slash command provided by the plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCommandDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// A hook event subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHookDef {
    /// Hook event name (e.g. "session_start", "tool_call_before").
    pub event: String,
    /// Execution priority (lower runs first).
    #[serde(default = "default_priority")]
    pub priority: i32,
}

fn default_priority() -> i32 {
    100
}

/// Lifecycle scripts for plugin init and shutdown.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginLifecycle {
    /// Script/command to run on plugin initialization.
    #[serde(default)]
    pub init: Option<String>,
    /// Script/command to run on plugin shutdown.
    #[serde(default)]
    pub shutdown: Option<String>,
}

impl PluginManifest {
    /// Parse a manifest from a TOML string.
    pub fn from_toml(content: &str) -> OxiResult<Self> {
        let manifest: Self = toml::from_str(content)
            .map_err(|e| OxiError::Config(format!("Invalid plugin.toml: {e}")))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Load a manifest from a file path.
    pub fn from_file(path: &Path) -> OxiResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| OxiError::Config(format!("Cannot read {}: {e}", path.display())))?;
        Self::from_toml(&content)
    }

    /// Validate manifest fields.
    fn validate(&self) -> OxiResult<()> {
        if self.name.is_empty() {
            return Err(OxiError::Config("Plugin name cannot be empty".into()));
        }
        // Prevent path traversal: only allow alphanumeric, hyphens, underscores.
        if !self.name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Err(OxiError::Config(format!(
                "Plugin name '{}' contains invalid characters (only [a-zA-Z0-9_-] allowed)",
                self.name
            )));
        }
        if self.command.is_empty() {
            return Err(OxiError::Config(
                format!("Plugin '{}' has no command", self.name),
            ));
        }
        // Ensure tool names are unique within the plugin.
        let mut seen = std::collections::HashSet::new();
        for tool in &self.tools {
            if !seen.insert(&tool.name) {
                return Err(OxiError::Config(
                    format!("Plugin '{}' has duplicate tool '{}'", self.name, tool.name),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_manifest() {
        let toml = r#"
name = "hello"
command = "node"
args = ["plugin.js"]
"#;
        let m = PluginManifest::from_toml(toml).unwrap();
        assert_eq!(m.name, "hello");
        assert_eq!(m.command, "node");
        assert_eq!(m.version, "0.1.0");
        assert!(m.tools.is_empty());
    }

    #[test]
    fn test_parse_full_manifest() {
        let toml = r#"
name = "example"
version = "1.0.0"
description = "An example plugin"
author = "test"
command = "python3"
args = ["-m", "example_plugin"]

[[tools]]
name = "greet"
description = "Say hello"

[[commands]]
name = "hello"
description = "Greet the user"

[[hooks]]
event = "session_start"
priority = 10

[lifecycle]
init = "echo init"
shutdown = "echo bye"
"#;
        let m = PluginManifest::from_toml(toml).unwrap();
        assert_eq!(m.tools.len(), 1);
        assert_eq!(m.commands.len(), 1);
        assert_eq!(m.hooks.len(), 1);
        assert_eq!(m.hooks[0].priority, 10);
        assert_eq!(m.lifecycle.init.as_deref(), Some("echo init"));
    }

    #[test]
    fn test_empty_name_rejected() {
        let toml = r#"
name = ""
command = "node"
"#;
        assert!(PluginManifest::from_toml(toml).is_err());
    }

    #[test]
    fn test_path_traversal_name_rejected() {
        let toml = r#"
name = "../../malicious"
command = "node"
"#;
        assert!(PluginManifest::from_toml(toml).is_err());
    }

    #[test]
    fn test_duplicate_tool_rejected() {
        let toml = r#"
name = "dup"
command = "node"

[[tools]]
name = "foo"

[[tools]]
name = "foo"
"#;
        assert!(PluginManifest::from_toml(toml).is_err());
    }
}
