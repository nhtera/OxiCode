//! `/settings-sync` — push/pull settings to/from the cloud (stub).
//!
//! Reads `~/.oxicode/settings.toml` to display a push summary.
//! Actual HTTP transport is not yet wired up; pull is a stub message.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

/// `/settings-sync [push|pull]` — synchronise local settings with the cloud.
pub struct SettingsSyncCommand;

impl SlashCommand for SettingsSyncCommand {
    fn name(&self) -> &str {
        "settings-sync"
    }

    fn description(&self) -> &str {
        "Sync settings to/from cloud (stub)"
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        match args.trim() {
            "push" => push_settings(ctx),
            "pull" => pull_settings(),
            "" => sync_status(ctx),
            other => CommandOutput::Error(format!(
                "Unknown subcommand: '{other}'\n\
                 Usage: /settings-sync [push|pull]\n\
                 No argument shows sync status."
            )),
        }
    }

    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["push", "pull"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Sub-command handlers
// ---------------------------------------------------------------------------

/// Read `settings.toml` and show a summary of what would be pushed.
fn push_settings(_ctx: &CommandContext) -> CommandOutput {
    let settings_path = match dirs::home_dir() {
        Some(h) => h.join(".oxicode").join("settings.toml"),
        None => return CommandOutput::Error("Could not determine home directory.".to_string()),
    };

    if !settings_path.exists() {
        return CommandOutput::Message(
            "No settings.toml found at ~/.oxicode/settings.toml\n\
             Create one first with your preferences."
                .to_string(),
        );
    }

    let content = match std::fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(e) => return CommandOutput::Error(format!("Failed to read settings.toml: {e}")),
    };

    // Count top-level TOML keys as a summary proxy.
    let key_count = content
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#') && t.contains('=')
        })
        .count();

    let section_count = content
        .lines()
        .filter(|l| {
            let t = l.trim();
            t.starts_with('[') && !t.starts_with("[[")
        })
        .count();

    let mut out = String::from("Settings push summary (stub — no HTTP yet):\n\n");
    let _ = writeln!(out, "  File:     {}", settings_path.display());
    let _ = writeln!(out, "  Sections: {section_count}");
    let _ = writeln!(out, "  Keys:     {key_count}");
    let _ = writeln!(out, "  Size:     {} bytes", content.len());
    out.push_str("\nTo enable cloud sync, configure [sync] in settings.toml with your API key.");

    tracing::debug!(
        "settings-sync push: {} keys, {} sections",
        key_count,
        section_count
    );
    CommandOutput::Message(out)
}

/// Stub pull — no HTTP transport yet.
fn pull_settings() -> CommandOutput {
    CommandOutput::Message(
        "Settings pull from cloud (stub):\n\n\
         No cloud sync endpoint configured.\n\
         To enable: add [sync] section to ~/.oxicode/settings.toml\n\n\
         Example:\n\
         [sync]\n\
         endpoint = \"https://sync.oxicode.dev\"\n\
         api_key  = \"your-api-key\""
            .to_string(),
    )
}

/// Show current sync status.
fn sync_status(_ctx: &CommandContext) -> CommandOutput {
    let settings_path = dirs::home_dir().map(|h| h.join(".oxicode").join("settings.toml"));

    let file_status = settings_path
        .as_ref()
        .map_or("unknown", |p| if p.exists() { "found" } else { "not found" });

    CommandOutput::Message(format!(
        "Settings sync status:\n\
         Local settings.toml: {file_status}\n\
         Cloud sync:          not configured (stub)\n\n\
         Subcommands: /settings-sync push | pull"
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_ctx() -> CommandContext {
        CommandContext {
            state_store: Arc::new(oxicode_state::StateStore::default()),
            model: "test".to_string(),
            provider_name: "test".to_string(),
            session_id: "test".to_string(),
        }
    }

    #[test]
    fn test_no_args_shows_status() {
        let cmd = SettingsSyncCommand;
        let ctx = make_ctx();
        match cmd.execute("", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("status")),
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn test_pull_stub_message() {
        let cmd = SettingsSyncCommand;
        let ctx = make_ctx();
        match cmd.execute("pull", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("stub")),
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn test_unknown_subcommand_error() {
        let cmd = SettingsSyncCommand;
        let ctx = make_ctx();
        match cmd.execute("upload", &ctx) {
            CommandOutput::Error(_) => {}
            _ => panic!("expected Error"),
        }
    }
}
