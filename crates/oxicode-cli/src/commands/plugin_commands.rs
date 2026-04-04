//! Plugin slash commands: /plugin [browse|search|info|install|update|remove|list].
//!
//! Provides marketplace integration (browse/search/info from remote registry),
//! local management (install/remove/list), and plugin update checks.

use std::fmt::Write as _;
use std::path::PathBuf;

use super::{CommandContext, CommandOutput, SlashCommand};

/// Default registry index URL (GitHub-hosted JSON).
/// Used when constructing PluginRegistry for async marketplace operations.
#[allow(dead_code)]
const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/nhtera/oxicode-plugins/main/index.json";

/// /plugin [subcommand] — manage plugins (marketplace + local).
pub struct PluginCommand;

impl SlashCommand for PluginCommand {
    fn name(&self) -> &str {
        "plugin"
    }
    fn description(&self) -> &str {
        "Manage plugins (browse/search/info/install/update/remove/list)"
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let plugins_dir = user_plugins_dir();

        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
        let rest = rest.trim();

        match sub.trim() {
            "browse" => execute_browse(rest),
            "search" => execute_search(rest),
            "info" => execute_info(rest),
            "install" => execute_install(rest, &plugins_dir),
            "update" => execute_update(rest),
            "remove" | "uninstall" => execute_remove(rest, &plugins_dir),
            "list" | "" => execute_list(&plugins_dir),
            other => CommandOutput::Error(format!(
                "Unknown: /plugin {other}\n\
                 Usage: /plugin [browse|search|info|install|update|remove|list]"
            )),
        }
    }

    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        [
            "browse", "search", "info", "install", "update", "remove", "list",
        ]
        .iter()
        .filter(|s| s.starts_with(partial))
        .map(|s| (*s).to_string())
        .collect()
    }
}

// --- Subcommand implementations ---

/// /plugin browse [page] — show paginated list of available plugins.
fn execute_browse(args: &str) -> CommandOutput {
    let page: usize = args.parse().unwrap_or(1);
    let page_size = 10;

    // Registry fetch is async; in sync command context, show cached or instructions.
    let cache_dir = registry_cache_dir();
    let cache_path = cache_dir.join("plugin-registry-cache.json");

    if !cache_path.exists() {
        return CommandOutput::Message(
            "No cached plugin index. Run a search first to fetch the registry:\n\
             /plugin search <query>\n\n\
             Or ask the assistant to browse plugins for you."
                .into(),
        );
    }

    match std::fs::read_to_string(&cache_path) {
        Ok(content) => {
            let cached: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => return CommandOutput::Error("Corrupted plugin cache.".into()),
            };

            let entries = cached
                .get("entries")
                .and_then(|e| e.as_array())
                .cloned()
                .unwrap_or_default();

            if entries.is_empty() {
                return CommandOutput::Message("No plugins in registry.".into());
            }

            let total = entries.len();
            let total_pages = (total + page_size - 1) / page_size;
            let start = (page.saturating_sub(1)) * page_size;

            if start >= total {
                return CommandOutput::Error(format!(
                    "Page {page} out of range (1-{total_pages})."
                ));
            }

            let end = (start + page_size).min(total);
            let mut output = format!(
                "Plugin Registry (page {page}/{total_pages}, {total} total):\n\n\
                 {:<25} {:<10} {:<12} {}\n\
                 {}\n",
                "NAME",
                "VERSION",
                "TRUST",
                "DESCRIPTION",
                "-".repeat(70)
            );

            for entry in &entries[start..end] {
                let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let version = entry.get("version").and_then(|v| v.as_str()).unwrap_or("?");
                let trust = entry
                    .get("trust")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unverified");
                let desc = entry
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                // Truncate description to 30 chars.
                let desc_short = if desc.len() > 30 {
                    format!("{}...", &desc[..27])
                } else {
                    desc.to_string()
                };
                let _ = writeln!(output, "{name:<25} {version:<10} {trust:<12} {desc_short}");
            }

            if page < total_pages {
                let _ = write!(output, "\nNext: /plugin browse {}", page + 1);
            }

            CommandOutput::Message(output)
        }
        Err(e) => CommandOutput::Error(format!("Failed to read plugin cache: {e}")),
    }
}

/// /plugin search <query> — search by name/keyword.
fn execute_search(query: &str) -> CommandOutput {
    if query.is_empty() {
        return CommandOutput::Error("Usage: /plugin search <query>".into());
    }

    let cache_dir = registry_cache_dir();
    let cache_path = cache_dir.join("plugin-registry-cache.json");

    if !cache_path.exists() {
        return CommandOutput::Message(format!(
            "No cached plugin index available.\n\
             Ask the assistant: \"Search for '{query}' plugins in the registry\"\n\
             This will fetch the index and search it."
        ));
    }

    match std::fs::read_to_string(&cache_path) {
        Ok(content) => {
            let cached: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => return CommandOutput::Error("Corrupted plugin cache.".into()),
            };

            let entries = cached
                .get("entries")
                .and_then(|e| e.as_array())
                .cloned()
                .unwrap_or_default();

            let q = query.to_lowercase();
            let matches: Vec<_> = entries
                .iter()
                .filter(|e| {
                    let name = e.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let desc = e.get("description").and_then(|v| v.as_str()).unwrap_or("");
                    let keywords = e
                        .get("keywords")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|k| k.as_str())
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default();
                    name.to_lowercase().contains(&q)
                        || desc.to_lowercase().contains(&q)
                        || keywords.to_lowercase().contains(&q)
                })
                .collect();

            if matches.is_empty() {
                return CommandOutput::Message(format!("No plugins matching '{query}'."));
            }

            let mut output = format!(
                "Search results for '{query}' ({} found):\n\n",
                matches.len()
            );
            for entry in &matches {
                let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let version = entry.get("version").and_then(|v| v.as_str()).unwrap_or("?");
                let trust = entry
                    .get("trust")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unverified");
                let desc = entry
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let _ = writeln!(output, "  {name} v{version} [{trust}] — {desc}");
            }
            let _ = write!(output, "\nInstall: /plugin install <name>");

            CommandOutput::Message(output)
        }
        Err(e) => CommandOutput::Error(format!("Failed to read plugin cache: {e}")),
    }
}

/// /plugin info <name> — show details + trust + permissions.
fn execute_info(name: &str) -> CommandOutput {
    if name.is_empty() {
        return CommandOutput::Error("Usage: /plugin info <name>".into());
    }

    // Check local installed plugin first.
    let plugins_dir = user_plugins_dir();
    let plugin_dir = plugins_dir.join(name);
    if plugin_dir.exists() {
        let manifest_path = plugin_dir.join("plugin.toml");
        if manifest_path.exists() {
            match std::fs::read_to_string(&manifest_path) {
                Ok(content) => {
                    return CommandOutput::Message(format!(
                        "Installed plugin: {name}\n\
                         Location: {}\n\n\
                         Manifest:\n{content}",
                        plugin_dir.display()
                    ));
                }
                Err(e) => {
                    return CommandOutput::Error(format!("Failed to read manifest: {e}"));
                }
            }
        }
    }

    // Check registry cache.
    let cache_path = registry_cache_dir().join("plugin-registry-cache.json");
    if cache_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&cache_path) {
            if let Ok(cached) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(entries) = cached.get("entries").and_then(|e| e.as_array()) {
                    let q = name.to_lowercase();
                    if let Some(entry) = entries.iter().find(|e| {
                        e.get("name")
                            .and_then(|v| v.as_str())
                            .is_some_and(|n| n.to_lowercase() == q)
                    }) {
                        return CommandOutput::Message(format_registry_entry(entry));
                    }
                }
            }
        }
    }

    CommandOutput::Message(format!(
        "Plugin '{name}' not found locally or in registry cache.\n\
         Try: /plugin search {name}"
    ))
}

/// /plugin install <path-or-name> — install from local path.
fn execute_install(target: &str, plugins_dir: &PathBuf) -> CommandOutput {
    if target.is_empty() {
        return CommandOutput::Error("Usage: /plugin install <path-or-name>".into());
    }

    let source = std::path::Path::new(target);
    if source.exists() && source.is_dir() {
        // Local directory install.
        let manifest_path = source.join("plugin.toml");
        if !manifest_path.exists() {
            return CommandOutput::Error(format!(
                "No plugin.toml found in {}\nPlugin directory must contain a plugin.toml manifest.",
                source.display()
            ));
        }

        std::fs::create_dir_all(plugins_dir).ok();
        match oxicode_plugins::install::install_plugin(source, plugins_dir) {
            Ok(installed) => CommandOutput::Message(format!(
                "Installed '{}' v{} to {}\n\
                 Run /reload-plugins to activate.",
                installed.name,
                installed.version,
                installed.dir.display()
            )),
            Err(e) => CommandOutput::Error(format!("Install failed: {e}")),
        }
    } else {
        // Assume registry name — instruct user to ask assistant.
        CommandOutput::Message(format!(
            "To install '{target}' from the registry:\n\
             Ask the assistant: \"Install the {target} plugin\"\n\
             This will download, verify, and install it.\n\n\
             For local install, provide a directory path: /plugin install ./path/to/plugin"
        ))
    }
}

/// /plugin update [name] — update installed plugins.
fn execute_update(name: &str) -> CommandOutput {
    if name.is_empty() {
        CommandOutput::Message(
            "Plugin update check:\n\
             Ask the assistant: \"Check for plugin updates\"\n\
             This will compare installed versions with the registry."
                .into(),
        )
    } else {
        CommandOutput::Message(format!(
            "To update '{name}':\n\
             Ask the assistant: \"Update the {name} plugin\"\n\
             This will check the registry and install the latest version."
        ))
    }
}

/// /plugin remove <name> — uninstall a plugin.
fn execute_remove(name: &str, plugins_dir: &PathBuf) -> CommandOutput {
    if name.is_empty() {
        return CommandOutput::Error("Usage: /plugin remove <name>".into());
    }

    match oxicode_plugins::install::uninstall_plugin(plugins_dir, name) {
        Ok(()) => CommandOutput::Message(format!(
            "Removed plugin '{name}'.\nRun /reload-plugins to update active plugins."
        )),
        Err(e) => CommandOutput::Error(format!("Failed to remove '{name}': {e}")),
    }
}

/// /plugin list — list installed plugins.
fn execute_list(plugins_dir: &PathBuf) -> CommandOutput {
    if !plugins_dir.exists() {
        return CommandOutput::Message(format!(
            "No plugins directory.\nCreate: {}",
            plugins_dir.display()
        ));
    }

    let installed = oxicode_plugins::install::discover_plugins(plugins_dir);
    if installed.is_empty() {
        return CommandOutput::Message("No plugins installed.".into());
    }

    let mut output = String::from("Installed plugins:\n\n");
    let _ = writeln!(output, "{:<20} {:<10} {}", "NAME", "VERSION", "DESCRIPTION");
    let _ = writeln!(output, "{}", "-".repeat(60));

    for plugin in &installed {
        let desc = if plugin.description.len() > 30 {
            format!("{}...", &plugin.description[..27])
        } else {
            plugin.description.clone()
        };
        let _ = writeln!(output, "{:<20} {:<10} {desc}", plugin.name, plugin.version);
    }

    CommandOutput::Message(output)
}

// --- Helpers ---

/// Format a registry entry for /plugin info display.
fn format_registry_entry(entry: &serde_json::Value) -> String {
    let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let version = entry.get("version").and_then(|v| v.as_str()).unwrap_or("?");
    let author = entry.get("author").and_then(|v| v.as_str()).unwrap_or("?");
    let desc = entry
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let trust = entry
        .get("trust")
        .and_then(|v| v.as_str())
        .unwrap_or("unverified");
    let dl_url = entry
        .get("download_url")
        .and_then(|v| v.as_str())
        .unwrap_or("N/A");
    let permissions = entry
        .get("permissions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "none".into());
    let keywords = entry
        .get("keywords")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    format!(
        "Plugin: {name} v{version}\n\
         Author: {author}\n\
         Trust: {trust}\n\
         Description: {desc}\n\
         Keywords: {keywords}\n\
         Permissions: {permissions}\n\
         Download: {dl_url}\n\n\
         Install: /plugin install {name}"
    )
}

/// User-level plugins directory.
fn user_plugins_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".oxicode")
        .join("plugins")
}

/// Registry cache directory.
fn registry_cache_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".oxicode")
        .join("cache")
}
