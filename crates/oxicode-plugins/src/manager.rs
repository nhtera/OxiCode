//! PluginManager: central coordinator for plugin discovery, loading, and execution.
//!
//! Discovers plugins from user and project directories, spawns subprocesses,
//! manages lifecycle, and dispatches tool/hook calls with security validation.
//! Supports hot-reload and registry-based installation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use oxicode_common::{OxiError, OxiResult};

use crate::install::{self, InstalledPlugin};
use crate::lifecycle;
use crate::manifest::{PluginManifest, PluginToolDef};
use crate::registry::PluginRegistry;
use crate::security;
use crate::subprocess::PluginSubprocess;

/// A loaded plugin with its subprocess and manifest.
struct LoadedPlugin {
    manifest: PluginManifest,
    dir: PathBuf,
    subprocess: Option<PluginSubprocess>,
}

/// Manages all plugins: discovery, loading, tool dispatch, hook dispatch.
pub struct PluginManager {
    plugins: HashMap<String, LoadedPlugin>,
    /// Project directory (for project-level plugin discovery).
    project_dir: Option<PathBuf>,
}

impl PluginManager {
    pub fn new(project_dir: Option<PathBuf>) -> Self {
        Self {
            plugins: HashMap::new(),
            project_dir,
        }
    }

    /// Discover and load all plugins from user and project directories.
    pub async fn discover_and_load(&mut self) -> Vec<String> {
        let dirs = install::plugin_dirs(self.project_dir.as_deref());
        let mut loaded = Vec::new();

        for dir in &dirs {
            let installed = install::discover_plugins(dir);
            for plugin in installed {
                match self.load_plugin(plugin).await {
                    Ok(()) => loaded.push(dir.display().to_string()),
                    Err(e) => tracing::error!("Failed to load plugin: {e}"),
                }
            }
        }

        loaded
    }

    /// Load a single discovered plugin: validate security, run init, spawn subprocess.
    async fn load_plugin(&mut self, installed: InstalledPlugin) -> OxiResult<()> {
        let name = &installed.name;

        if self.plugins.contains_key(name) {
            return Err(OxiError::Config(format!("Plugin '{name}' already loaded")));
        }

        // Validate tool names against builtins.
        for tool in &installed.manifest.tools {
            security::validate_tool_name(name, &tool.name)?;
        }

        // Run init lifecycle script if declared.
        if let Some(init_script) = &installed.manifest.lifecycle.init {
            lifecycle::run_lifecycle_script(name, init_script, &installed.dir, "init").await?;
        }

        // Spawn the plugin subprocess.
        let env: Vec<(String, String)> = installed
            .manifest
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let subprocess = PluginSubprocess::spawn(
            name,
            &installed.manifest.command,
            &installed.manifest.args,
            &env,
        )?;

        tracing::info!(
            "Plugin '{name}' loaded: {} tools, {} hooks",
            installed.manifest.tools.len(),
            installed.manifest.hooks.len()
        );

        self.plugins.insert(
            name.clone(),
            LoadedPlugin {
                manifest: installed.manifest,
                dir: installed.dir,
                subprocess: Some(subprocess),
            },
        );

        Ok(())
    }

    /// Call a tool provided by a plugin.
    pub async fn call_tool(
        &self,
        plugin_name: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> OxiResult<serde_json::Value> {
        security::validate_method(plugin_name, "tool/call")?;

        let plugin = self
            .plugins
            .get(plugin_name)
            .ok_or_else(|| OxiError::Other(format!("Plugin '{plugin_name}' not loaded")))?;

        // Verify the tool exists in the manifest.
        if !plugin.manifest.tools.iter().any(|t| t.name == tool_name) {
            return Err(OxiError::Tool {
                name: tool_name.to_string(),
                message: format!("Tool '{tool_name}' not found in plugin '{plugin_name}'"),
            });
        }

        let subprocess = plugin.subprocess.as_ref().ok_or_else(|| {
            OxiError::Other(format!("Plugin '{plugin_name}' subprocess not running"))
        })?;

        subprocess.call_tool(tool_name, arguments).await
    }

    /// Dispatch a hook event to all plugins that subscribe to it.
    pub async fn dispatch_hook(
        &self,
        event: &str,
        data: serde_json::Value,
    ) -> Vec<(String, OxiResult<serde_json::Value>)> {
        let mut results = Vec::new();

        // Collect subscribers sorted by priority.
        let mut subscribers: Vec<(&str, i32)> = Vec::new();
        for (name, plugin) in &self.plugins {
            for hook in &plugin.manifest.hooks {
                if hook.event == event {
                    subscribers.push((name.as_str(), hook.priority));
                }
            }
        }
        subscribers.sort_by_key(|(_, priority)| *priority);

        for (name, _) in subscribers {
            let plugin = &self.plugins[name];
            if let Some(subprocess) = &plugin.subprocess {
                let result = subprocess.dispatch_hook(event, data.clone()).await;
                results.push((name.to_string(), result));
            }
        }

        results
    }

    /// Get all tools across all loaded plugins, prefixed with plugin name.
    pub fn all_tools(&self) -> Vec<(String, &PluginToolDef)> {
        let mut tools = Vec::new();
        for (name, plugin) in &self.plugins {
            for tool in &plugin.manifest.tools {
                let prefixed = format!("{name}__{}", tool.name);
                tools.push((prefixed, tool));
            }
        }
        tools
    }

    /// Resolve a prefixed tool name ("plugin__tool") to (plugin_name, tool_name).
    pub fn resolve_tool_name(prefixed: &str) -> Option<(&str, &str)> {
        prefixed.split_once("__")
    }

    /// List loaded plugin names.
    pub fn plugin_names(&self) -> Vec<&str> {
        self.plugins.keys().map(String::as_str).collect()
    }

    /// Get manifest for a loaded plugin.
    pub fn get_manifest(&self, name: &str) -> Option<&PluginManifest> {
        self.plugins.get(name).map(|p| &p.manifest)
    }

    /// Number of loaded plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Install a plugin from a source directory.
    pub fn install(&self, source_dir: &Path) -> OxiResult<InstalledPlugin> {
        let target = self.user_plugins_dir()?;
        std::fs::create_dir_all(&target)?;
        install::install_plugin(source_dir, &target)
    }

    /// Uninstall a plugin by name.
    pub fn uninstall(&mut self, name: &str) -> OxiResult<()> {
        let target = self.user_plugins_dir()?;
        install::uninstall_plugin(&target, name)?;
        self.plugins.remove(name);
        Ok(())
    }

    /// Hot-reload all plugins: shut down running plugins, re-discover, re-load.
    /// Returns names of successfully reloaded plugins.
    pub async fn reload_plugins(&mut self) -> Vec<String> {
        tracing::info!("Hot-reloading plugins...");

        // Shut down all current plugins.
        self.shutdown_all().await;
        self.plugins.clear();

        // Re-discover and load.
        self.discover_and_load().await
    }

    /// Install a plugin from the remote registry by name.
    /// Downloads the plugin archive, extracts it, and loads it.
    pub async fn install_from_registry(
        &mut self,
        name: &str,
        registry: &PluginRegistry,
    ) -> OxiResult<InstalledPlugin> {
        let entries = registry.fetch_index().await?;
        let entry = entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| OxiError::Config(format!("Plugin '{name}' not found in registry")))?;

        // Check trust level — reject Unverified, warn for Community.
        let trust = security::assess_trust(&entry.trust);
        if trust == security::TrustLevel::Unverified {
            return Err(OxiError::Permission(format!(
                "Plugin '{}' is unverified and cannot be auto-installed. \
                 Use /plugin install <local-path> for manual installs.",
                entry.name
            )));
        }
        if trust.requires_approval() {
            tracing::warn!(
                "Plugin '{}' has trust level: {}. Community plugins require caution.",
                entry.name,
                trust
            );
        }

        // Download the plugin archive.
        let archive_bytes = registry.download_plugin(entry).await?;

        // Extract to user plugins directory.
        let target_dir = self.user_plugins_dir()?;
        std::fs::create_dir_all(&target_dir)?;

        let plugin_dir = target_dir.join(&entry.name);
        if plugin_dir.exists() {
            // Remove old version before installing new.
            std::fs::remove_dir_all(&plugin_dir)
                .map_err(|e| OxiError::Other(format!("Failed to remove old version: {e}")))?;
        }

        // Write archive bytes to temp file and extract.
        // For now, assume the archive is a tar.gz.
        Self::extract_tar_gz(&archive_bytes, &plugin_dir)?;

        // Discover and load the newly installed plugin.
        let installed = install::discover_plugins(&target_dir);
        let plugin = installed
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                OxiError::Config(format!("Plugin '{name}' installed but manifest not found"))
            })?;

        // Load it into the manager.
        let result = plugin.clone();
        self.load_plugin(plugin).await?;

        tracing::info!(
            "Installed plugin '{}' v{} from registry",
            result.name,
            result.version
        );
        Ok(result)
    }

    /// Extract a tar.gz archive to the target directory.
    /// Includes path traversal protection: rejects entries that escape target dir.
    /// Limits total extracted size to 100MB to prevent OOM.
    fn extract_tar_gz(data: &[u8], target: &Path) -> OxiResult<()> {
        use std::io::Read;

        const MAX_EXTRACT_SIZE: u64 = 100 * 1024 * 1024; // 100 MB

        let gz = flate2::read::GzDecoder::new(data);
        let mut archive = tar::Archive::new(gz);

        std::fs::create_dir_all(target)?;
        let canonical_target = target
            .canonicalize()
            .map_err(|e| OxiError::Other(format!("Cannot canonicalize target dir: {e}")))?;

        let mut total_size: u64 = 0;

        // Extract all entries, stripping the top-level directory if present.
        for entry_result in archive
            .entries()
            .map_err(|e| OxiError::Other(format!("Failed to read archive entries: {e}")))?
        {
            let mut entry =
                entry_result.map_err(|e| OxiError::Other(format!("Bad archive entry: {e}")))?;
            let path = entry
                .path()
                .map_err(|e| OxiError::Other(format!("Bad path in archive: {e}")))?
                .into_owned();

            // Strip first component (top-level dir in tarball).
            let stripped: PathBuf = path.components().skip(1).collect();
            if stripped.as_os_str().is_empty() {
                continue;
            }

            // SECURITY: Reject path traversal attempts (e.g. "../../etc/passwd").
            let dest = target.join(&stripped);
            let canonical_dest = dest.canonicalize().unwrap_or_else(|_| {
                // File doesn't exist yet — normalize manually by resolving parent.
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).ok();
                    parent
                        .canonicalize()
                        .map(|p| p.join(dest.file_name().unwrap_or_default()))
                        .unwrap_or_else(|_| dest.clone())
                } else {
                    dest.clone()
                }
            });

            if !canonical_dest.starts_with(&canonical_target) {
                return Err(OxiError::Permission(format!(
                    "Path traversal detected in plugin archive: {}",
                    stripped.display()
                )));
            }

            // Size limit check.
            total_size += entry.header().size().unwrap_or(0);
            if total_size > MAX_EXTRACT_SIZE {
                return Err(OxiError::Other(
                    "Plugin archive exceeds 100MB extraction limit".into(),
                ));
            }

            if entry.header().entry_type().is_dir() {
                std::fs::create_dir_all(&dest)?;
            } else {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut buf = Vec::new();
                entry
                    .read_to_end(&mut buf)
                    .map_err(|e| OxiError::Other(format!("Failed to read entry: {e}")))?;
                std::fs::write(&dest, &buf)?;
            }
        }

        Ok(())
    }

    /// Shut down all loaded plugins gracefully.
    pub async fn shutdown_all(&mut self) {
        for (name, plugin) in &mut self.plugins {
            // Run shutdown lifecycle script.
            if let Some(shutdown_script) = &plugin.manifest.lifecycle.shutdown {
                if let Err(e) =
                    lifecycle::run_lifecycle_script(name, shutdown_script, &plugin.dir, "shutdown")
                        .await
                {
                    tracing::warn!("Plugin '{name}' shutdown script failed: {e}");
                }
            }

            // Shut down subprocess.
            if let Some(subprocess) = plugin.subprocess.take() {
                subprocess.shutdown().await;
            }

            tracing::info!("Plugin '{name}' shut down");
        }
    }

    /// User-level plugins directory.
    fn user_plugins_dir(&self) -> OxiResult<PathBuf> {
        dirs::home_dir()
            .map(|h| h.join(".oxicode").join("plugins"))
            .ok_or_else(|| OxiError::Config("Cannot determine home directory".into()))
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_manager() {
        let mgr = PluginManager::new(None);
        assert_eq!(mgr.plugin_count(), 0);
        assert!(mgr.plugin_names().is_empty());
    }

    #[test]
    fn test_resolve_tool_name() {
        assert_eq!(
            PluginManager::resolve_tool_name("hello__greet"),
            Some(("hello", "greet"))
        );
        assert_eq!(PluginManager::resolve_tool_name("noprefix"), None);
    }

    #[test]
    fn test_all_tools_empty() {
        let mgr = PluginManager::new(None);
        assert!(mgr.all_tools().is_empty());
    }
}
