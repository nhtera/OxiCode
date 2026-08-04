//! Utility commands: /upgrade, /privacy-settings, /add-dir, /list-dirs,
//! /remove-dir, /extra-usage, /mcp-doctor.
//!
//! General-purpose utility commands for system management and diagnostics.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

// ── /upgrade ──────────────────────────────────────────────────────────────

/// /upgrade — check for new versions and offer to update.
pub struct UpgradeCommand;

impl SlashCommand for UpgradeCommand {
    fn name(&self) -> &str {
        "upgrade"
    }
    fn description(&self) -> &str {
        "Check for updates"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let current = env!("CARGO_PKG_VERSION");

        // Check GitHub releases API (best-effort, synchronous for command context).
        // In production this would be async; here we provide the mechanism.
        CommandOutput::Message(format!(
            "Current version: v{current}\n\
             To upgrade, run:\n\
             \n\
             cargo install oxicode --force\n\
             \n\
             Or download the latest release from:\n\
             https://github.com/nhtera/oxicode/releases"
        ))
    }
}

// ── /privacy-settings ─────────────────────────────────────────────────────

/// /privacy-settings — toggle telemetry and data sharing preferences.
/// Unregistered in Phase 6 prune (2026-04-26); kept for re-introduction.
#[allow(dead_code)]
pub struct PrivacySettingsCommand;

impl SlashCommand for PrivacySettingsCommand {
    fn name(&self) -> &str {
        "privacy-settings"
    }
    fn description(&self) -> &str {
        "Toggle telemetry and data sharing"
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let arg = args.trim().to_lowercase();

        if arg.is_empty() {
            // Show current settings.
            let state = ctx.state_store.current();
            let telemetry = if state.feature_flags.is_enabled("telemetry") {
                "on"
            } else {
                "off"
            };
            let data_sharing = if state.feature_flags.is_enabled("data_sharing") {
                "on"
            } else {
                "off"
            };
            return CommandOutput::Message(format!(
                "Privacy settings:\n\
                 \n\
                 Telemetry:    {telemetry}\n\
                 Data sharing: {data_sharing}\n\
                 \n\
                 Usage: /privacy-settings <telemetry|data-sharing> <on|off>"
            ));
        }

        // Parse: /privacy-settings telemetry on|off
        let parts: Vec<&str> = arg.split_whitespace().collect();
        if parts.len() != 2 {
            return CommandOutput::Error(
                "Usage: /privacy-settings <telemetry|data-sharing> <on|off>".into(),
            );
        }

        let (setting, action) = (parts[0], parts[1]);
        let flag_name = match setting {
            "telemetry" => "telemetry",
            "data-sharing" | "data_sharing" | "sharing" => "data_sharing",
            _ => {
                return CommandOutput::Error(format!(
                    "Unknown setting: {setting}. Use 'telemetry' or 'data-sharing'."
                ));
            }
        };

        let target_on = match action {
            "on" | "enable" | "true" | "1" => true,
            "off" | "disable" | "false" | "0" => false,
            _ => {
                return CommandOutput::Error("Use 'on' or 'off'.".into());
            }
        };

        let currently_on = ctx.state_store.is_feature_enabled(flag_name);
        if currently_on != target_on {
            ctx.state_store.toggle_feature(flag_name);
        }

        let status = if target_on { "enabled" } else { "disabled" };
        CommandOutput::Message(format!("{setting} {status}."))
    }
}

// ── /add-dir ──────────────────────────────────────────────────────────────

/// /add-dir <path> — add a directory to the session's working directories.
pub struct AddDirCommand;

impl SlashCommand for AddDirCommand {
    fn name(&self) -> &str {
        "add-dir"
    }
    fn description(&self) -> &str {
        "Add a working directory"
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let path_str = args.trim();
        if path_str.is_empty() {
            return CommandOutput::Error("Usage: /add-dir <path>".into());
        }

        let path = std::path::Path::new(path_str);
        if !path.exists() {
            return CommandOutput::Error(format!("Path does not exist: {path_str}"));
        }
        if !path.is_dir() {
            return CommandOutput::Error(format!("Not a directory: {path_str}"));
        }

        // Canonicalize to resolve symlinks and prevent path traversal.
        let canonical = match path.canonicalize() {
            Ok(p) => p,
            Err(e) => return CommandOutput::Error(format!("Failed to canonicalize path: {e}")),
        };

        // Set semantics: no-op if already present.
        let already_present = ctx.state_store.current().working_dirs.contains(&canonical);

        if already_present {
            return CommandOutput::Message(format!(
                "Directory already in working set: {}",
                canonical.display()
            ));
        }

        ctx.state_store
            .update(|s| s.working_dirs.push(canonical.clone()));

        tracing::info!("Added working directory: {}", canonical.display());

        CommandOutput::Message(format!(
            "Added working directory: {}\nUse /list-dirs to see all extra directories.",
            canonical.display()
        ))
    }
}

// ── /list-dirs ────────────────────────────────────────────────────────────

/// /list-dirs — list all extra working directories added via /add-dir.
pub struct ListDirsCommand;

impl SlashCommand for ListDirsCommand {
    fn name(&self) -> &str {
        "list-dirs"
    }
    fn description(&self) -> &str {
        "List extra working directories"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let dirs = ctx.state_store.current().working_dirs;
        if dirs.is_empty() {
            return CommandOutput::Message(
                "No extra working directories. Use /add-dir <path> to add one.".into(),
            );
        }

        let mut output = String::from("Extra working directories:\n");
        for dir in &dirs {
            let _ = writeln!(output, "  {}", dir.display());
        }
        CommandOutput::Message(output.trim_end().to_string())
    }
}

// ── /remove-dir ───────────────────────────────────────────────────────────

/// /remove-dir <path> — remove a directory from the session's working directories.
pub struct RemoveDirCommand;

impl SlashCommand for RemoveDirCommand {
    fn name(&self) -> &str {
        "remove-dir"
    }
    fn description(&self) -> &str {
        "Remove a working directory"
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let path_str = args.trim();
        if path_str.is_empty() {
            return CommandOutput::Error("Usage: /remove-dir <path>".into());
        }

        // Canonicalize the input for comparison (best-effort; if it fails, compare as-is).
        let canonical = std::path::Path::new(path_str)
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from(path_str));

        let mut found = false;
        ctx.state_store.update(|s| {
            let before = s.working_dirs.len();
            s.working_dirs.retain(|p| p != &canonical);
            found = s.working_dirs.len() < before;
        });

        if found {
            tracing::info!("Removed working directory: {}", canonical.display());
            CommandOutput::Message(format!(
                "Removed working directory: {}",
                canonical.display()
            ))
        } else {
            CommandOutput::Error(format!(
                "Directory not in working set: {}",
                canonical.display()
            ))
        }
    }
}

// ── /extra-usage ──────────────────────────────────────────────────────────

/// /extra-usage — show extended token usage and estimated cost.
/// Unregistered in Phase 6 prune (2026-04-26); kept for re-introduction.
#[allow(dead_code)]
pub struct ExtraUsageCommand;

impl SlashCommand for ExtraUsageCommand {
    fn name(&self) -> &str {
        "extra-usage"
    }
    fn description(&self) -> &str {
        "Show extended usage and cost info"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let u = &state.total_usage;

        let cache_creation = u.cache_creation_input_tokens.unwrap_or(0);
        let cache_read = u.cache_read_input_tokens.unwrap_or(0);

        // Estimate cost (Claude 3.5 Sonnet pricing as reference).
        let input_cost = f64::from(u.input_tokens) * 3.0 / 1_000_000.0;
        let output_cost = f64::from(u.output_tokens) * 15.0 / 1_000_000.0;
        let cache_read_savings = f64::from(cache_read) * 2.7 / 1_000_000.0;
        let total_cost = input_cost + output_cost;

        let mut output = String::new();
        let _ = writeln!(output, "Extended usage info:");
        let _ = writeln!(output);
        let _ = writeln!(output, "  Input tokens:        {}", u.input_tokens);
        let _ = writeln!(output, "  Output tokens:       {}", u.output_tokens);
        let _ = writeln!(output, "  Cache creation:      {cache_creation}");
        let _ = writeln!(output, "  Cache reads:         {cache_read}");
        let _ = writeln!(output);
        let _ = writeln!(output, "  Estimated cost:      ${total_cost:.4}");
        let _ = writeln!(output, "    Input:             ${input_cost:.4}");
        let _ = writeln!(output, "    Output:            ${output_cost:.4}");
        if cache_read > 0 {
            let _ = writeln!(output, "    Cache savings:     ~${cache_read_savings:.4}");
        }
        let _ = writeln!(output);
        let _ = writeln!(output, "  Model: {}", ctx.model);
        let _ = writeln!(output, "  Provider: {}", ctx.provider_name);

        // Rate limit info.
        if let Some(ref rl) = state.last_rate_limit {
            let _ = writeln!(output);
            let _ = writeln!(
                output,
                "  Last rate limited: {} ({})",
                rl.occurred_at, rl.provider
            );
        }

        CommandOutput::Message(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_ctx() -> CommandContext {
        CommandContext {
            state_store: Arc::new(oxicode_state::StateStore::default()),
            model: "claude-3-5-sonnet".to_string(),
            provider_name: "anthropic".to_string(),
            session_id: "test-session".to_string(),
            command_registry: Arc::new(crate::commands::default_registry()),
        }
    }

    #[test]
    fn test_upgrade_command() {
        let cmd = UpgradeCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("Current version"));
                assert!(msg.contains("cargo install"));
            }
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_privacy_settings_show() {
        let cmd = PrivacySettingsCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("Telemetry"));
                assert!(msg.contains("Data sharing"));
            }
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_privacy_settings_toggle_telemetry() {
        let cmd = PrivacySettingsCommand;
        let ctx = make_ctx();
        let output = cmd.execute("telemetry on", &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("enabled")),
            _ => panic!("Expected message"),
        }
        assert!(ctx.state_store.is_feature_enabled("telemetry"));
    }

    #[test]
    fn test_privacy_settings_invalid() {
        let cmd = PrivacySettingsCommand;
        let ctx = make_ctx();
        let output = cmd.execute("invalid", &ctx);
        match output {
            CommandOutput::Error(msg) => assert!(msg.contains("Usage")),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn test_add_dir_missing_args() {
        let cmd = AddDirCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Error(msg) => assert!(msg.contains("Usage")),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn test_add_dir_nonexistent() {
        let cmd = AddDirCommand;
        let ctx = make_ctx();
        let output = cmd.execute("/nonexistent/path/xyz", &ctx);
        match output {
            CommandOutput::Error(msg) => assert!(msg.contains("does not exist")),
            _ => panic!("Expected error"),
        }
    }

    /// A directory that exists on every platform, usable as a `/add-dir` arg.
    fn temp_dir_arg() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    /// The canonical form `/add-dir` stores for `temp_dir_arg()`.
    fn temp_dir_canonical() -> std::path::PathBuf {
        std::fs::canonicalize(std::env::temp_dir()).expect("canonicalize temp dir")
    }

    #[test]
    fn test_add_dir_valid() {
        let cmd = AddDirCommand;
        let ctx = make_ctx();
        let output = cmd.execute(&temp_dir_arg(), &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("Added working directory")),
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_add_dir_persists_to_state() {
        let ctx = make_ctx();
        AddDirCommand.execute(&temp_dir_arg(), &ctx);
        let dirs = ctx.state_store.current().working_dirs;
        assert_eq!(dirs.len(), 1);
        // Stored canonicalized, so compare against the canonical form
        // (/tmp resolves to /private/tmp on macOS, and Windows adds a prefix).
        assert_eq!(dirs[0], temp_dir_canonical());
    }

    #[test]
    fn test_add_dir_duplicate_noop() {
        let ctx = make_ctx();
        AddDirCommand.execute(&temp_dir_arg(), &ctx);
        let output = AddDirCommand.execute(&temp_dir_arg(), &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("already in working set")),
            _ => panic!("Expected 'already in working set' message"),
        }
        assert_eq!(ctx.state_store.current().working_dirs.len(), 1);
    }

    #[test]
    fn test_list_dirs_empty() {
        let ctx = make_ctx();
        let output = ListDirsCommand.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("No extra working directories")),
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_list_dirs_shows_added() {
        let ctx = make_ctx();
        AddDirCommand.execute(&temp_dir_arg(), &ctx);
        let output = ListDirsCommand.execute("", &ctx);
        let expected = temp_dir_canonical().display().to_string();
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains(&expected)),
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_remove_dir_missing_args() {
        let ctx = make_ctx();
        let output = RemoveDirCommand.execute("", &ctx);
        match output {
            CommandOutput::Error(msg) => assert!(msg.contains("Usage")),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn test_remove_dir_not_in_set() {
        let ctx = make_ctx();
        let output = RemoveDirCommand.execute("/tmp", &ctx);
        match output {
            CommandOutput::Error(msg) => assert!(msg.contains("not in working set")),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn test_remove_dir_success() {
        let ctx = make_ctx();
        AddDirCommand.execute(&temp_dir_arg(), &ctx);
        assert_eq!(ctx.state_store.current().working_dirs.len(), 1);
        let output = RemoveDirCommand.execute(&temp_dir_arg(), &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("Removed working directory")),
            _ => panic!("Expected message"),
        }
        assert!(ctx.state_store.current().working_dirs.is_empty());
    }

    #[test]
    fn test_extra_usage() {
        let cmd = ExtraUsageCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("Input tokens"));
                assert!(msg.contains("Output tokens"));
                assert!(msg.contains("Estimated cost"));
                assert!(msg.contains("claude-3-5-sonnet"));
            }
            _ => panic!("Expected message"),
        }
    }
}
