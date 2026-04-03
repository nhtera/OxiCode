use oxicode_common::FeatureFlags;
use serde::{Deserialize, Serialize};

/// Application settings loaded from TOML, env vars, and defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            api_key: None,
            model: oxicode_common::constants::DEFAULT_MODEL.to_string(),
            max_tokens: oxicode_common::constants::DEFAULT_MAX_TOKENS,
            theme: "default".to_string(),
            permission_mode: "default".to_string(),
            config_dir: None,
            features: FeatureFlags::default(),
        }
    }
}
