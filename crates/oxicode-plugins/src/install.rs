//! Plugin install/uninstall/list operations.
//!
//! Plugins are directories containing a `plugin.toml` manifest.
//! Install copies a plugin directory into the user or project plugin path.

use std::path::{Path, PathBuf};

use oxicode_common::{OxiError, OxiResult};

use crate::manifest::PluginManifest;

/// Installed plugin entry with path and manifest.
#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    pub name: String,
    pub version: String,
    pub description: String,
    pub dir: PathBuf,
    pub manifest: PluginManifest,
}

/// Discover installed plugins by scanning a directory for `plugin.toml` files.
pub fn discover_plugins(plugins_dir: &Path) -> Vec<InstalledPlugin> {
    let mut plugins = Vec::new();

    let Ok(entries) = std::fs::read_dir(plugins_dir) else {
        return plugins;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let manifest_path = path.join("plugin.toml");
        if !manifest_path.exists() {
            continue;
        }

        match PluginManifest::from_file(&manifest_path) {
            Ok(manifest) => {
                plugins.push(InstalledPlugin {
                    name: manifest.name.clone(),
                    version: manifest.version.clone(),
                    description: manifest.description.clone(),
                    dir: path,
                    manifest,
                });
            }
            Err(e) => {
                tracing::warn!(
                    "Skipping invalid plugin at {}: {e}",
                    manifest_path.display()
                );
            }
        }
    }

    plugins
}

/// Install a plugin from a source directory into the target plugins directory.
/// Copies the entire source directory as a subdirectory named after the plugin.
pub fn install_plugin(source_dir: &Path, target_plugins_dir: &Path) -> OxiResult<InstalledPlugin> {
    let manifest_path = source_dir.join("plugin.toml");
    let manifest = PluginManifest::from_file(&manifest_path)?;

    let dest = target_plugins_dir.join(&manifest.name);
    if dest.exists() {
        return Err(OxiError::Config(format!(
            "Plugin '{}' already installed at {}",
            manifest.name,
            dest.display()
        )));
    }

    copy_dir_recursive(source_dir, &dest)?;

    Ok(InstalledPlugin {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        dir: dest,
        manifest,
    })
}

/// Uninstall a plugin by removing its directory.
pub fn uninstall_plugin(plugins_dir: &Path, plugin_name: &str) -> OxiResult<()> {
    let target_dir = plugins_dir.join(plugin_name);
    if !target_dir.exists() {
        return Err(OxiError::Config(format!(
            "Plugin '{plugin_name}' not found in {}",
            plugins_dir.display()
        )));
    }

    std::fs::remove_dir_all(&target_dir)
        .map_err(|e| OxiError::Other(format!("Failed to remove plugin '{plugin_name}': {e}")))
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> OxiResult<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Return plugin directories: user-level and project-level.
pub fn plugin_dirs(project_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // User-level: ~/.oxicode/plugins/
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".oxicode").join("plugins"));
    }

    // Project-level: .oxicode/plugins/
    if let Some(proj) = project_dir {
        dirs.push(proj.join(".oxicode").join("plugins"));
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_discover_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = discover_plugins(dir.path());
        assert!(plugins.is_empty());
    }

    #[test]
    fn test_discover_valid_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let plugin_dir = dir.path().join("hello");
        fs::create_dir(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            r#"name = "hello"
command = "node"
args = ["index.js"]
"#,
        )
        .unwrap();

        let plugins = discover_plugins(dir.path());
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "hello");
    }

    #[test]
    fn test_plugin_dirs() {
        let dirs = plugin_dirs(Some(Path::new("/tmp/project")));
        assert!(!dirs.is_empty());
        assert!(dirs.last().unwrap().ends_with(".oxicode/plugins"));
    }
}
