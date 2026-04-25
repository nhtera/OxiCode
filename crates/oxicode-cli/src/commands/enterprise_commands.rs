//! Enterprise slash commands: /telemetry, /settings, /auth, /managed.
//!
//! These commands provide enterprise-grade features for team/org adoption:
//! telemetry observability, settings import/export, auth management,
//! and MDM managed settings inspection.

use std::fmt::Write;

use super::{CommandContext, CommandOutput, SlashCommand};

/// /telemetry [on|off|stats|reset] — manage telemetry collection.
pub struct TelemetryCommand;
impl SlashCommand for TelemetryCommand {
    fn name(&self) -> &str {
        "telemetry"
    }
    fn description(&self) -> &str {
        "Toggle telemetry or show stats"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        match args.trim() {
            "on" => {
                crate::telemetry::set_telemetry_enabled(true);
                CommandOutput::Message("Telemetry enabled.".into())
            }
            "off" => {
                crate::telemetry::set_telemetry_enabled(false);
                CommandOutput::Message("Telemetry disabled.".into())
            }
            "toggle" | "" => {
                let new_state = crate::telemetry::toggle_telemetry();
                CommandOutput::Message(format!(
                    "Telemetry {}.",
                    if new_state { "enabled" } else { "disabled" }
                ))
            }
            "stats" => {
                let collector = crate::telemetry::global_collector();
                CommandOutput::Message(collector.summary())
            }
            "reset" => {
                let collector = crate::telemetry::global_collector();
                collector.reset();
                CommandOutput::Message("Telemetry metrics reset.".into())
            }
            other => CommandOutput::Error(format!(
                "Unknown: /telemetry {other}\nUsage: /telemetry [on|off|toggle|stats|reset]"
            )),
        }
    }
    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["on", "off", "toggle", "stats", "reset"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}

/// /settings [export|import] — manage settings export/import.
pub struct SettingsCommand;
impl SlashCommand for SettingsCommand {
    fn name(&self) -> &str {
        "settings"
    }
    fn description(&self) -> &str {
        "Export or import settings"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args.trim(), ""));
        match sub {
            "export" => {
                let settings = oxicode_config::load_settings(None);
                let path = if rest.trim().is_empty() {
                    "settings-export.json".to_string()
                } else {
                    rest.trim().to_string()
                };
                match oxicode_config::sync::export_to_file(&settings, std::path::Path::new(&path)) {
                    Ok(()) => CommandOutput::Message(format!("Settings exported to: {path}")),
                    Err(e) => CommandOutput::Error(format!("Export failed: {e}")),
                }
            }
            "import" => {
                let path = rest.trim();
                if path.is_empty() {
                    return CommandOutput::Error("Usage: /settings import <file.json>".into());
                }
                match oxicode_config::sync::import_from_file(std::path::Path::new(path)) {
                    Ok(result) => {
                        // Persist imported settings to settings.toml.
                        let config_dir = oxicode_config::config_dir(None);
                        let toml_path = config_dir.join("settings.toml");
                        let persist_msg = match toml::to_string_pretty(&result.settings) {
                            Ok(toml_str) => match std::fs::write(&toml_path, toml_str) {
                                Ok(()) => format!("\nSaved to: {}", toml_path.display()),
                                Err(e) => format!("\n⚠ Failed to save: {e}"),
                            },
                            Err(e) => {
                                format!("\n⚠ Failed to serialize: {e}")
                            }
                        };

                        let mut msg = format!("Settings imported from: {path}{persist_msg}");
                        if !result.warnings.is_empty() {
                            msg.push_str("\n\nWarnings:");
                            for w in &result.warnings {
                                let _ = write!(msg, "\n  ⚠ {w}");
                            }
                        }
                        CommandOutput::Message(msg)
                    }
                    Err(e) => CommandOutput::Error(format!("Import failed: {e}")),
                }
            }
            "show" | "" => {
                let settings = oxicode_config::load_settings(None);
                match oxicode_config::sync::export_settings(&settings) {
                    Ok(json) => CommandOutput::Message(format!("Current settings:\n{json}")),
                    Err(e) => CommandOutput::Error(format!("Failed to display: {e}")),
                }
            }
            other => CommandOutput::Error(format!(
                "Unknown: /settings {other}\nUsage: /settings [show|export|import]"
            )),
        }
    }
    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["show", "export", "import"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}

/// /auth [status|login|logout] — manage authentication.
pub struct AuthCommand;
impl SlashCommand for AuthCommand {
    fn name(&self) -> &str {
        "auth"
    }
    fn description(&self) -> &str {
        "Manage authentication and credentials"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let (sub, rest) = args.split_once(' ').unwrap_or((args.trim(), ""));
        match sub {
            "status" | "" => {
                let mgr = crate::auth::AuthManager::new();
                CommandOutput::Message(format!("Authentication status:\n{}", mgr.status_summary()))
            }
            "login" => {
                let provider = if rest.trim().is_empty() {
                    &ctx.provider_name
                } else {
                    rest.trim()
                };

                // Check if already authenticated.
                let mgr = crate::auth::AuthManager::new();
                if mgr.is_authenticated(provider) {
                    return CommandOutput::Message(format!(
                        "Already authenticated with {provider}."
                    ));
                }

                CommandOutput::Message(format!(
                    "To authenticate with {provider}:\n\
                     1. Set the API key: export {}=<your-key>\n\
                     2. Or add to: ~/.oxicode/credentials.toml\n\
                     \n\
                     OAuth flow available for: anthropic",
                    match provider {
                        "anthropic" => "ANTHROPIC_API_KEY",
                        "openai" => "OPENAI_API_KEY",
                        "google" | "gemini" => "GOOGLE_API_KEY",
                        _ => "<PROVIDER_API_KEY>",
                    }
                ))
            }
            "logout" => {
                let provider = if rest.trim().is_empty() {
                    &ctx.provider_name
                } else {
                    rest.trim()
                };
                let mut mgr = crate::auth::AuthManager::new();
                match mgr.clear_credential(provider) {
                    Ok(()) => CommandOutput::Message(format!(
                        "Credentials cleared for {provider}.\n\
                         Note: Environment variables (if set) are still active."
                    )),
                    Err(e) => CommandOutput::Error(format!("Logout failed: {e}")),
                }
            }
            other => CommandOutput::Error(format!(
                "Unknown: /auth {other}\nUsage: /auth [status|login|logout]"
            )),
        }
    }
    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["status", "login", "logout"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}

/// /managed — show MDM managed settings.
/// Unregistered in Phase 6 prune (2026-04-26); kept for re-introduction.
#[allow(dead_code)]
pub struct ManagedCommand;
impl SlashCommand for ManagedCommand {
    fn name(&self) -> &str {
        "managed"
    }
    fn description(&self) -> &str {
        "Show MDM managed settings"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let managed = oxicode_config::mdm::load_managed_settings();
        CommandOutput::Message(managed.summary())
    }
}
