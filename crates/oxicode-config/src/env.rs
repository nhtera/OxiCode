use oxicode_common::constants;

use crate::settings::Settings;

/// Apply environment variable overrides to settings.
pub fn apply_env_overrides(settings: &mut Settings) {
    // API key: ANTHROPIC_API_KEY takes priority, ANTHROPIC_AUTH_TOKEN as fallback.
    if let Ok(key) = std::env::var(constants::ENV_API_KEY) {
        if !key.is_empty() {
            settings.api_key = Some(key);
        }
    } else if let Ok(token) = std::env::var(constants::ENV_AUTH_TOKEN) {
        if !token.is_empty() {
            settings.api_key = Some(token);
        }
    }

    if let Ok(model) = std::env::var(constants::ENV_MODEL) {
        if !model.is_empty() {
            settings.model = model;
        }
    }

    if let Ok(dir) = std::env::var(constants::ENV_CONFIG_DIR) {
        if !dir.is_empty() {
            settings.config_dir = Some(dir);
        }
    }

    // Model alias env vars (used by ProviderRouter for shorthand resolution).
    if let Ok(m) = std::env::var(constants::ENV_DEFAULT_HAIKU_MODEL) {
        if !m.is_empty() {
            settings.default_haiku_model = Some(m);
        }
    }
    if let Ok(m) = std::env::var(constants::ENV_DEFAULT_SONNET_MODEL) {
        if !m.is_empty() {
            settings.default_sonnet_model = Some(m);
        }
    }
    if let Ok(m) = std::env::var(constants::ENV_DEFAULT_OPUS_MODEL) {
        if !m.is_empty() {
            settings.default_opus_model = Some(m);
        }
    }
}
