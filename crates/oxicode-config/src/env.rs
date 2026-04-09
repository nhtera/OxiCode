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

    let mut model_overridden_by_env = false;
    if let Ok(model) = std::env::var(constants::ENV_MODEL) {
        if !model.is_empty() {
            settings.model = model;
            model_overridden_by_env = true;
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

    // If user did not explicitly set OXICODE_MODEL and config still uses the
    // built-in default, honor ANTHROPIC_DEFAULT_SONNET_MODEL as runtime default.
    if !model_overridden_by_env
        && settings.model == constants::DEFAULT_MODEL
        && settings.default_sonnet_model.is_some()
    {
        settings.model = settings.default_sonnet_model.clone().unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn set_or_remove(key: &str, value: Option<&str>) {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn test_auth_token_fallback_sets_api_key() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let api_key_prev = std::env::var(constants::ENV_API_KEY).ok();
        let token_prev = std::env::var(constants::ENV_AUTH_TOKEN).ok();

        set_or_remove(constants::ENV_API_KEY, None);
        set_or_remove(constants::ENV_AUTH_TOKEN, Some("token-from-auth-token"));

        let mut settings = Settings::default();
        apply_env_overrides(&mut settings);
        assert_eq!(settings.api_key.as_deref(), Some("token-from-auth-token"));

        set_or_remove(constants::ENV_API_KEY, api_key_prev.as_deref());
        set_or_remove(constants::ENV_AUTH_TOKEN, token_prev.as_deref());
    }

    #[test]
    fn test_default_sonnet_model_overrides_builtin_default() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let model_prev = std::env::var(constants::ENV_MODEL).ok();
        let sonnet_prev = std::env::var(constants::ENV_DEFAULT_SONNET_MODEL).ok();

        set_or_remove(constants::ENV_MODEL, None);
        set_or_remove(
            constants::ENV_DEFAULT_SONNET_MODEL,
            Some("claude-sonnet-4.6"),
        );

        let mut settings = Settings::default();
        assert_eq!(settings.model, constants::DEFAULT_MODEL);
        apply_env_overrides(&mut settings);
        assert_eq!(settings.model, "claude-sonnet-4.6");

        set_or_remove(constants::ENV_MODEL, model_prev.as_deref());
        set_or_remove(constants::ENV_DEFAULT_SONNET_MODEL, sonnet_prev.as_deref());
    }

    #[test]
    fn test_explicit_model_env_wins_over_default_sonnet() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let model_prev = std::env::var(constants::ENV_MODEL).ok();
        let sonnet_prev = std::env::var(constants::ENV_DEFAULT_SONNET_MODEL).ok();

        set_or_remove(constants::ENV_MODEL, Some("claude-opus-4.6"));
        set_or_remove(
            constants::ENV_DEFAULT_SONNET_MODEL,
            Some("claude-sonnet-4.6"),
        );

        let mut settings = Settings::default();
        apply_env_overrides(&mut settings);
        assert_eq!(settings.model, "claude-opus-4.6");

        set_or_remove(constants::ENV_MODEL, model_prev.as_deref());
        set_or_remove(constants::ENV_DEFAULT_SONNET_MODEL, sonnet_prev.as_deref());
    }
}
