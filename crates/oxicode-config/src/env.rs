use oxicode_common::constants;

use crate::settings::Settings;

/// Apply environment variable overrides to settings.
pub fn apply_env_overrides(settings: &mut Settings) {
    if let Ok(key) = std::env::var(constants::ENV_API_KEY) {
        if !key.is_empty() {
            settings.api_key = Some(key);
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
}
