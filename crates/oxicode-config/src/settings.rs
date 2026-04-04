use oxicode_common::FeatureFlags;
use serde::{Deserialize, Serialize};

/// Application settings loaded from TOML, env vars, and defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Schema version — tracks which config migrations have been applied.
    pub config_version: u32,
    /// Anthropic API key.
    pub api_key: Option<String>,
    /// Model ID to use.
    pub model: String,
    /// Max tokens for response.
    pub max_tokens: u32,
    /// Theme name for TUI.
    pub theme: String,
    /// Permission mode: "default", "`accept_edits`", "bypass".
    pub permission_mode: String,
    /// Custom config directory override.
    pub config_dir: Option<String>,
    /// Runtime feature flags.
    pub features: FeatureFlags,
    /// Editor mode: "normal" or "vim".
    pub editor_mode: String,
    /// Output rendering style: "plain", "markdown", "minimal", "verbose".
    pub output_style: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            config_version: 0,
            api_key: None,
            model: oxicode_common::constants::DEFAULT_MODEL.to_string(),
            max_tokens: oxicode_common::constants::DEFAULT_MAX_TOKENS,
            theme: "default".to_string(),
            permission_mode: "default".to_string(),
            config_dir: None,
            features: FeatureFlags::default(),
            editor_mode: "normal".to_string(),
            output_style: "markdown".to_string(),
        }
    }
}
