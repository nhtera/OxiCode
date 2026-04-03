//! Plugin slash commands: /plugin install, /plugin list, /plugin remove.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

/// /plugin [install|list|remove] — manage plugins via filesystem.
pub struct PluginCommand;

impl SlashCommand for PluginCommand {
    fn name(&self) -> &str {
        "plugin"
    }
    fn description(&self) -> &str {
        "Manage plugins (install/list/remove)"
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let plugins_dir = dirs::home_dir()
            .map(|h| h.join(".oxicode").join("plugins"))
            .unwrap_or_default();

        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        match sub.trim() {
            "install" => {
                if rest.trim().is_empty() {
                    CommandOutput::Error("Usage: /plugin install <path-or-name>".into())
                } else {
                    CommandOutput::Message(format!(
                        "To install a plugin:\n\
                         1. Copy the plugin directory to {}\n\
                         2. Restart OxiCode\n\n\
                         Plugin must contain a manifest.toml file.",
                        plugins_dir.display()
                    ))
                }
            }
            "remove" | "uninstall" => {
                if rest.trim().is_empty() {
                    CommandOutput::Error("Usage: /plugin remove <name>".into())
                } else {
                    CommandOutput::Message(format!(
                        "To remove '{}':\n\
                         Delete the directory: {}/{}\n\
                         Then restart OxiCode.",
                        rest.trim(),
                        plugins_dir.display(),
                        rest.trim()
                    ))
                }
            }
            "list" | "" => {
                if !plugins_dir.exists() {
                    return CommandOutput::Message(format!(
                        "No plugins directory.\nCreate: {}",
                        plugins_dir.display()
                    ));
                }
                match std::fs::read_dir(&plugins_dir) {
                    Ok(entries) => {
                        let dirs: Vec<_> = entries
                            .filter_map(Result::ok)
                            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                            .collect();
                        if dirs.is_empty() {
                            CommandOutput::Message("No plugins installed.".into())
                        } else {
                            let mut output = String::from("Installed plugins:\n");
                            for entry in dirs {
                                let name = entry.file_name();
                                let has_manifest =
                                    entry.path().join("manifest.toml").exists();
                                let status = if has_manifest { "valid" } else { "no manifest" };
                                let _ =
                                    writeln!(output, "  {:<20} [{status}]", name.to_string_lossy());
                            }
                            CommandOutput::Message(output)
                        }
                    }
                    Err(e) => CommandOutput::Error(format!("Failed to read plugins dir: {e}")),
                }
            }
            other => CommandOutput::Error(format!(
                "Unknown: /plugin {other}. Use: install, list, remove"
            )),
        }
    }

    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["install", "list", "remove"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}
