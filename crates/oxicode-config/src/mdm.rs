//! MDM (Mobile Device Management) / managed settings loader.
//!
//! Loads organization-enforced settings from platform-specific sources:
//! - macOS: plist domain `com.oxicode.settings`
//! - Linux: `/etc/oxicode/managed.toml`
//! - Windows: registry key `HKLM\Software\OxiCode`
//!
//! MDM settings have highest precedence and cannot be overridden by users.

use std::collections::HashMap;
use std::path::Path;

/// A managed setting with its value and lock status.
#[derive(Debug, Clone)]
pub struct ManagedSetting {
    /// The setting key (e.g. "model", "permission_mode").
    pub key: String,
    /// The setting value as a string.
    pub value: String,
    /// Whether this setting is locked (cannot be overridden by user).
    pub locked: bool,
}

/// Collection of managed settings from MDM source.
#[derive(Debug, Clone, Default)]
pub struct ManagedSettings {
    /// All managed settings keyed by name.
    pub settings: HashMap<String, ManagedSetting>,
    /// Source description (e.g. "macOS plist", "/etc/oxicode/managed.toml").
    pub source: Option<String>,
}

impl ManagedSettings {
    /// Check if any managed settings were loaded.
    pub fn is_empty(&self) -> bool {
        self.settings.is_empty()
    }

    /// Check if a setting is managed (present in MDM).
    pub fn is_managed(&self, key: &str) -> bool {
        self.settings.contains_key(key)
    }

    /// Check if a setting is locked (managed + cannot be overridden).
    pub fn is_locked(&self, key: &str) -> bool {
        self.settings.get(key).is_some_and(|s| s.locked)
    }

    /// Get the value of a managed setting.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.settings.get(key).map(|s| s.value.as_str())
    }

    /// List all managed settings.
    pub fn list_all(&self) -> Vec<(&str, &str, bool)> {
        let mut items: Vec<_> = self
            .settings
            .values()
            .map(|s| (s.key.as_str(), s.value.as_str(), s.locked))
            .collect();
        items.sort_by_key(|(k, _, _)| *k);
        items
    }

    /// Format a human-readable summary.
    pub fn summary(&self) -> String {
        if self.is_empty() {
            return "No managed settings active.".to_string();
        }

        let source = self
            .source
            .as_deref()
            .unwrap_or("unknown");
        let mut lines = vec![format!("Managed settings (source: {source}):")];

        for (key, value, locked) in self.list_all() {
            let lock_icon = if locked { " [locked]" } else { "" };
            lines.push(format!("  {key} = {value}{lock_icon}"));
        }

        lines.join("\n")
    }
}

/// Load managed settings from the platform-appropriate source.
pub fn load_managed_settings() -> ManagedSettings {
    // Try platform-specific sources in order.
    #[cfg(target_os = "macos")]
    {
        if let Some(settings) = load_macos_plist() {
            return settings;
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(settings) = load_windows_registry() {
            return settings;
        }
    }

    // Linux and fallback: check /etc/oxicode/managed.toml
    if let Some(settings) = load_managed_toml(Path::new("/etc/oxicode/managed.toml")) {
        return settings;
    }

    ManagedSettings::default()
}

/// Load managed settings from a TOML file (Linux + cross-platform fallback).
///
/// Expected format:
/// ```toml
/// [settings]
/// model = "claude-sonnet-4-20250514"
/// permission_mode = "default"
///
/// [locked]
/// model = true
/// permission_mode = true
/// ```
fn load_managed_toml(path: &Path) -> Option<ManagedSettings> {
    let content = std::fs::read_to_string(path).ok()?;
    let table: toml::Table = content.parse().ok()?;

    let settings_table = table.get("settings")?.as_table()?;
    let locked_table = table.get("locked").and_then(|v| v.as_table());

    let mut managed = ManagedSettings {
        settings: HashMap::new(),
        source: Some(path.display().to_string()),
    };

    for (key, value) in settings_table {
        let value_str = match value {
            toml::Value::String(s) => s.clone(),
            toml::Value::Integer(i) => i.to_string(),
            toml::Value::Boolean(b) => b.to_string(),
            toml::Value::Float(f) => f.to_string(),
            _ => continue,
        };

        let locked = locked_table
            .and_then(|lt| lt.get(key))
            .and_then(toml::Value::as_bool)
            .unwrap_or(true); // MDM settings are locked by default.

        managed.settings.insert(
            key.clone(),
            ManagedSetting {
                key: key.clone(),
                value: value_str,
                locked,
            },
        );
    }

    if managed.is_empty() {
        None
    } else {
        Some(managed)
    }
}

/// macOS: read from `defaults` command for domain `com.oxicode.settings`.
#[cfg(target_os = "macos")]
fn load_macos_plist() -> Option<ManagedSettings> {
    use std::process::Command;

    let output = Command::new("defaults")
        .args(["read", "com.oxicode.settings"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut managed = ManagedSettings {
        settings: HashMap::new(),
        source: Some("macOS plist (com.oxicode.settings)".into()),
    };

    // Parse the plist output: "key = value;" format.
    for line in stdout.lines() {
        let line = line.trim().trim_end_matches(';').trim();
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().trim_matches('"').to_string();
            let value = value.trim().trim_matches('"').to_string();
            managed.settings.insert(
                key.clone(),
                ManagedSetting {
                    key,
                    value,
                    locked: true, // macOS MDM settings are always locked.
                },
            );
        }
    }

    if managed.is_empty() {
        None
    } else {
        Some(managed)
    }
}

/// Windows: read from registry `HKLM\Software\OxiCode`.
#[cfg(target_os = "windows")]
fn load_windows_registry() -> Option<ManagedSettings> {
    use std::process::Command;

    let output = Command::new("reg")
        .args(["query", r"HKLM\Software\OxiCode", "/s"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut managed = ManagedSettings {
        settings: HashMap::new(),
        source: Some(r"Windows registry (HKLM\Software\OxiCode)".into()),
    };

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // Format: "    name    REG_SZ    value"
        if parts.len() >= 3 && parts[1].starts_with("REG_") {
            let key = parts[0].to_string();
            let value = parts[2..].join(" ");
            managed.settings.insert(
                key.clone(),
                ManagedSetting {
                    key,
                    value,
                    locked: true,
                },
            );
        }
    }

    if managed.is_empty() {
        None
    } else {
        Some(managed)
    }
}

/// Apply managed settings to a settings struct, respecting lock precedence.
/// Returns list of keys that were overridden.
pub fn apply_managed_settings(
    settings: &mut crate::Settings,
    managed: &ManagedSettings,
) -> Vec<String> {
    let mut overridden = Vec::new();

    if let Some(model) = managed.get("model") {
        settings.model = model.to_string();
        overridden.push("model".into());
    }
    if let Some(mode) = managed.get("permission_mode") {
        settings.permission_mode = mode.to_string();
        overridden.push("permission_mode".into());
    }
    if let Some(theme) = managed.get("theme") {
        settings.theme = theme.to_string();
        overridden.push("theme".into());
    }
    if let Some(editor_mode) = managed.get("editor_mode") {
        settings.editor_mode = editor_mode.to_string();
        overridden.push("editor_mode".into());
    }
    if let Some(max_tokens) = managed.get("max_tokens") {
        if let Ok(val) = max_tokens.parse::<u32>() {
            settings.max_tokens = val;
            overridden.push("max_tokens".into());
        }
    }
    if let Some(output_style) = managed.get("output_style") {
        settings.output_style = output_style.to_string();
        overridden.push("output_style".into());
    }

    overridden
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_managed_settings() {
        let ms = ManagedSettings::default();
        assert!(ms.is_empty());
        assert!(!ms.is_managed("model"));
        assert!(!ms.is_locked("model"));
        assert!(ms.summary().contains("No managed settings"));
    }

    #[test]
    fn test_managed_setting_lookup() {
        let mut ms = ManagedSettings::default();
        ms.settings.insert(
            "model".into(),
            ManagedSetting {
                key: "model".into(),
                value: "claude-opus-4-20250514".into(),
                locked: true,
            },
        );
        assert!(ms.is_managed("model"));
        assert!(ms.is_locked("model"));
        assert_eq!(ms.get("model"), Some("claude-opus-4-20250514"));
    }

    #[test]
    fn test_managed_setting_not_locked() {
        let mut ms = ManagedSettings::default();
        ms.settings.insert(
            "theme".into(),
            ManagedSetting {
                key: "theme".into(),
                value: "dark".into(),
                locked: false,
            },
        );
        assert!(ms.is_managed("theme"));
        assert!(!ms.is_locked("theme"));
    }

    #[test]
    fn test_load_managed_toml() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            r#"
[settings]
model = "claude-opus-4-20250514"
permission_mode = "bypass"
max_tokens = 8192

[locked]
model = true
permission_mode = true
max_tokens = false
"#,
        )
        .unwrap();

        let ms = load_managed_toml(tmp.path()).unwrap();
        assert_eq!(ms.get("model"), Some("claude-opus-4-20250514"));
        assert!(ms.is_locked("model"));
        assert!(ms.is_locked("permission_mode"));
        assert!(!ms.is_locked("max_tokens"));
    }

    #[test]
    fn test_load_managed_toml_missing_file() {
        let result = load_managed_toml(Path::new("/nonexistent/managed.toml"));
        assert!(result.is_none());
    }

    #[test]
    fn test_summary_format() {
        let mut ms = ManagedSettings {
            settings: HashMap::new(),
            source: Some("test".into()),
        };
        ms.settings.insert(
            "model".into(),
            ManagedSetting {
                key: "model".into(),
                value: "opus".into(),
                locked: true,
            },
        );
        let summary = ms.summary();
        assert!(summary.contains("model = opus [locked]"));
        assert!(summary.contains("source: test"));
    }

    #[test]
    fn test_apply_managed_settings() {
        let mut settings = crate::Settings::default();
        let mut ms = ManagedSettings::default();
        ms.settings.insert(
            "model".into(),
            ManagedSetting {
                key: "model".into(),
                value: "claude-opus-4-20250514".into(),
                locked: true,
            },
        );
        ms.settings.insert(
            "max_tokens".into(),
            ManagedSetting {
                key: "max_tokens".into(),
                value: "4096".into(),
                locked: true,
            },
        );

        let overridden = apply_managed_settings(&mut settings, &ms);
        assert_eq!(settings.model, "claude-opus-4-20250514");
        assert_eq!(settings.max_tokens, 4096);
        assert!(overridden.contains(&"model".to_string()));
        assert!(overridden.contains(&"max_tokens".to_string()));
    }

    #[test]
    fn test_list_all_sorted() {
        let mut ms = ManagedSettings::default();
        ms.settings.insert(
            "theme".into(),
            ManagedSetting {
                key: "theme".into(),
                value: "dark".into(),
                locked: false,
            },
        );
        ms.settings.insert(
            "model".into(),
            ManagedSetting {
                key: "model".into(),
                value: "opus".into(),
                locked: true,
            },
        );
        let all = ms.list_all();
        assert_eq!(all[0].0, "model");
        assert_eq!(all[1].0, "theme");
    }
}
