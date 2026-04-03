pub mod claude_md;
pub mod env;
pub mod settings;

use std::path::{Path, PathBuf};

use oxicode_common::constants;
pub use settings::Settings;

/// Resolve the config directory path.
/// Priority: explicit override > env var > default (~/.oxicode/).
pub fn config_dir(override_dir: Option<&str>) -> PathBuf {
    if let Some(dir) = override_dir {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var(constants::ENV_CONFIG_DIR) {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(constants::CONFIG_DIR_NAME)
}

/// Load settings with full 3-level merge: defaults < TOML file < env vars.
pub fn load_settings(config_dir_override: Option<&str>) -> Settings {
    let dir = config_dir(config_dir_override);
    let config_path = dir.join(constants::CONFIG_FILE_NAME);

    // Start with defaults
    let mut settings = Settings::default();

    // Layer TOML file if it exists
    if config_path.is_file() {
        match std::fs::read_to_string(&config_path) {
            Ok(content) => match toml::from_str::<Settings>(&content) {
                Ok(file_settings) => {
                    merge_settings(&mut settings, &file_settings);
                    tracing::debug!("Loaded config from {}", config_path.display());
                }
                Err(e) => {
                    tracing::warn!("Failed to parse {}: {}", config_path.display(), e);
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read {}: {}", config_path.display(), e);
            }
        }
    }

    // Layer env vars (highest priority)
    env::apply_env_overrides(&mut settings);

    settings
}

/// Merge non-default values from source into target.
fn merge_settings(target: &mut Settings, source: &Settings) {
    if source.api_key.is_some() {
        target.api_key.clone_from(&source.api_key);
    }
    let defaults = Settings::default();
    if source.model != defaults.model {
        target.model.clone_from(&source.model);
    }
    if source.max_tokens != defaults.max_tokens {
        target.max_tokens = source.max_tokens;
    }
    if source.theme != defaults.theme {
        target.theme.clone_from(&source.theme);
    }
    if source.permission_mode != defaults.permission_mode {
        target.permission_mode.clone_from(&source.permission_mode);
    }
    if source.editor_mode != defaults.editor_mode {
        target.editor_mode.clone_from(&source.editor_mode);
    }
    if source.output_style != defaults.output_style {
        target.output_style.clone_from(&source.output_style);
    }
    // Always take features from TOML — they are fully deserialized with defaults.
    target.features = source.features.clone();
}

/// Load all CLAUDE.md / OXICODE.md content for the given working directory.
/// Returns (`global_md`, `project_md`) — either may be None.
pub fn load_claude_md(cwd: &Path) -> (Option<String>, Option<String>) {
    let global = claude_md::discover_global_claude_md();
    let project = claude_md::discover_claude_md(cwd).map(|(_, content)| content);
    (global, project)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let s = Settings::default();
        assert!(s.api_key.is_none());
        assert_eq!(s.model, constants::DEFAULT_MODEL);
    }

    #[test]
    fn test_config_dir_default() {
        let dir = config_dir(None);
        assert!(dir.to_string_lossy().contains(constants::CONFIG_DIR_NAME));
    }

    #[test]
    fn test_config_dir_override() {
        let dir = config_dir(Some("/tmp/my-config"));
        assert_eq!(dir, PathBuf::from("/tmp/my-config"));
    }

    #[test]
    fn test_load_settings_from_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let toml_content = r#"
model = "claude-opus-4-20250514"
max_tokens = 8192
"#;
        std::fs::write(tmp.path().join("settings.toml"), toml_content).unwrap();
        let settings = load_settings(Some(tmp.path().to_str().unwrap()));
        assert_eq!(settings.model, "claude-opus-4-20250514");
        assert_eq!(settings.max_tokens, 8192);
    }

    #[test]
    fn test_load_settings_with_features() {
        let tmp = tempfile::tempdir().unwrap();
        let toml_content = r#"
model = "claude-sonnet-4-20250514"

[features]
extended_thinking = false
proactive_agents = true
vim_mode = true
custom_flag = true
"#;
        std::fs::write(tmp.path().join("settings.toml"), toml_content).unwrap();
        let settings = load_settings(Some(tmp.path().to_str().unwrap()));
        assert!(!settings.features.is_enabled("extended_thinking"));
        assert!(settings.features.is_enabled("proactive_agents"));
        assert!(settings.features.is_enabled("vim_mode"));
        assert!(settings.features.is_enabled("custom_flag"));
        // Default features not overridden should keep defaults.
        assert!(settings.features.is_enabled("prompt_caching"));
    }

    #[test]
    fn test_default_settings_have_default_features() {
        let s = Settings::default();
        assert!(s.features.is_enabled("extended_thinking"));
        assert!(s.features.is_enabled("prompt_caching"));
        assert!(!s.features.is_enabled("proactive_agents"));
    }
}
