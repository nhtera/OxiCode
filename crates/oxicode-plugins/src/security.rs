//! Plugin security sandbox enforcement.
//!
//! Plugins run as separate processes (OS-level isolation). This module enforces
//! additional restrictions: plugins cannot set permission modes, modify hooks,
//! or access other plugins' state.

use oxicode_common::{OxiError, OxiResult};

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
}
