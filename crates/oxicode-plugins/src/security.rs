//! Plugin security sandbox enforcement.
//!
//! Plugins run as separate processes (OS-level isolation). This module enforces
//! additional restrictions: plugins cannot set permission modes, modify hooks,
//! or access other plugins' state. Also provides trust-level assessment for
//! registry plugins.

use std::fmt;

use oxicode_common::{OxiError, OxiResult};
use serde::{Deserialize, Serialize};

/// Trust level for a plugin, derived from registry metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustLevel {
    /// Verified by the OxiCode team or a trusted publisher.
    Verified,
    /// Published by a community member with a known identity.
    Community,
    /// Unknown or untrusted source.
    Unverified,
}

impl TrustLevel {
    /// Parse from a string (case-insensitive).
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "verified" => Self::Verified,
            "community" => Self::Community,
            _ => Self::Unverified,
        }
    }

    /// Whether user confirmation is required before install.
    pub fn requires_approval(&self) -> bool {
        matches!(self, Self::Community | Self::Unverified)
    }

    /// ANSI-colored badge for TUI display.
    pub fn badge(&self) -> &'static str {
        match self {
            Self::Verified => "[verified]",
            Self::Community => "[community]",
            Self::Unverified => "[unverified]",
        }
    }

    /// Color hint for TUI rendering (green/yellow/red).
    pub fn color_hint(&self) -> &'static str {
        match self {
            Self::Verified => "green",
            Self::Community => "yellow",
            Self::Unverified => "red",
        }
    }
}

impl fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verified => write!(f, "verified"),
            Self::Community => write!(f, "community"),
            Self::Unverified => write!(f, "unverified"),
        }
    }
}

/// Assess trust level from registry entry metadata.
pub fn assess_trust(trust_field: &str) -> TrustLevel {
    TrustLevel::from_str_loose(trust_field)
}

/// Permission manifest summary: what a plugin requests access to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionManifest {
    /// Tool names the plugin registers.
    pub tools: Vec<String>,
    /// Hook events the plugin subscribes to.
    pub hooks: Vec<String>,
    /// Slash commands the plugin provides.
    pub commands: Vec<String>,
}

impl PermissionManifest {
    /// Build from a plugin manifest.
    pub fn from_manifest(manifest: &crate::manifest::PluginManifest) -> Self {
        Self {
            tools: manifest.tools.iter().map(|t| t.name.clone()).collect(),
            hooks: manifest.hooks.iter().map(|h| h.event.clone()).collect(),
            commands: manifest.commands.iter().map(|c| c.name.clone()).collect(),
        }
    }

    /// Format a human-readable summary for review.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.tools.is_empty() {
            parts.push(format!("Tools: {}", self.tools.join(", ")));
        }
        if !self.hooks.is_empty() {
            parts.push(format!("Hooks: {}", self.hooks.join(", ")));
        }
        if !self.commands.is_empty() {
            parts.push(format!("Commands: {}", self.commands.join(", ")));
        }
        if parts.is_empty() {
            "No permissions requested.".to_string()
        } else {
            parts.join("\n")
        }
    }
}

/// Actions that plugins are forbidden from performing.
const FORBIDDEN_METHODS: &[&str] = &[
    "permission/set",
    "permission/grant",
    "hooks/modify",
    "hooks/register",
    "hooks/unregister",
    "plugin/access",
    "config/set_permission_mode",
];

/// Validate that a plugin request method is allowed by the sandbox.
pub fn validate_method(plugin_name: &str, method: &str) -> OxiResult<()> {
    if FORBIDDEN_METHODS.contains(&method) {
        return Err(OxiError::Permission(format!(
            "Plugin '{plugin_name}' attempted forbidden method: {method}"
        )));
    }
    Ok(())
}

/// Validate that plugin-provided tool names don't collide with builtins.
/// Uses prefix matching: a plugin cannot register "bash_extended" either.
pub fn validate_tool_name(plugin_name: &str, tool_name: &str) -> OxiResult<()> {
    const RESERVED_PREFIXES: &[&str] = &[
        "bash",
        "file_read",
        "file_write",
        "file_edit",
        "glob",
        "grep",
        "notebook_edit",
        "agent",
        "mcp",
        "send_message",
        "ask_user",
        "tool_search",
        "config",
        "enter_plan_mode",
        "exit_plan_mode",
        "enter_worktree",
        "exit_worktree",
        "sleep",
        "remote_trigger",
        "cron_",
        "brief",
        "structured_output",
    ];

    for prefix in RESERVED_PREFIXES {
        if tool_name == *prefix || tool_name.starts_with(&format!("{prefix}_")) {
            return Err(OxiError::Permission(format!(
                "Plugin '{plugin_name}' cannot shadow builtin tool '{tool_name}'"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_method() {
        assert!(validate_method("test", "tool/call").is_ok());
        assert!(validate_method("test", "hook/dispatch").is_ok());
    }

    #[test]
    fn test_forbidden_method() {
        assert!(validate_method("test", "permission/set").is_err());
        assert!(validate_method("test", "hooks/modify").is_err());
    }

    #[test]
    fn test_allowed_tool_name() {
        assert!(validate_tool_name("test", "my_custom_tool").is_ok());
    }

    #[test]
    fn test_reserved_tool_name() {
        assert!(validate_tool_name("test", "bash").is_err());
        assert!(validate_tool_name("test", "file_read").is_err());
        // Prefix matching: bash_extended is also blocked.
        assert!(validate_tool_name("test", "bash_extended").is_err());
    }

    #[test]
    fn test_trust_level_from_str() {
        assert_eq!(TrustLevel::from_str_loose("verified"), TrustLevel::Verified);
        assert_eq!(TrustLevel::from_str_loose("Verified"), TrustLevel::Verified);
        assert_eq!(TrustLevel::from_str_loose("community"), TrustLevel::Community);
        assert_eq!(TrustLevel::from_str_loose("COMMUNITY"), TrustLevel::Community);
        assert_eq!(TrustLevel::from_str_loose("unverified"), TrustLevel::Unverified);
        assert_eq!(TrustLevel::from_str_loose("unknown"), TrustLevel::Unverified);
        assert_eq!(TrustLevel::from_str_loose(""), TrustLevel::Unverified);
    }

    #[test]
    fn test_trust_level_requires_approval() {
        assert!(!TrustLevel::Verified.requires_approval());
        assert!(TrustLevel::Community.requires_approval());
        assert!(TrustLevel::Unverified.requires_approval());
    }

    #[test]
    fn test_trust_level_badge() {
        assert_eq!(TrustLevel::Verified.badge(), "[verified]");
        assert_eq!(TrustLevel::Community.badge(), "[community]");
        assert_eq!(TrustLevel::Unverified.badge(), "[unverified]");
    }

    #[test]
    fn test_assess_trust() {
        assert_eq!(assess_trust("verified"), TrustLevel::Verified);
        assert_eq!(assess_trust("community"), TrustLevel::Community);
        assert_eq!(assess_trust("garbage"), TrustLevel::Unverified);
    }

    #[test]
    fn test_permission_manifest_summary() {
        let pm = PermissionManifest {
            tools: vec!["greet".into(), "calc".into()],
            hooks: vec!["session_start".into()],
            commands: vec![],
        };
        let summary = pm.summary();
        assert!(summary.contains("Tools: greet, calc"));
        assert!(summary.contains("Hooks: session_start"));
        assert!(!summary.contains("Commands"));
    }

    #[test]
    fn test_permission_manifest_empty() {
        let pm = PermissionManifest::default();
        assert_eq!(pm.summary(), "No permissions requested.");
    }
}
