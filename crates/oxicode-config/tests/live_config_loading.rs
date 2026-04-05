//! Integration tests for config loading, env overrides, and CLAUDE.md discovery.
//!
//! Tests the full 3-level merge: defaults < TOML file < env vars.
//! Also tests CLAUDE.md / OXICODE.md discovery from project directories.
//!
//! Run with: `cargo test -p oxicode-config --test live_config_loading`

use oxicode_config::{load_settings, Settings};
use oxicode_config::claude_md::discover_claude_md;
use std::fs;

// ── Env Override Integration ────────────────────────────────────
// NOTE: Env var tests must run serially (--test-threads=1) since they mutate
// process-wide state. Each test saves and restores env vars, but parallel
// execution can still race.

#[test]
fn test_env_override_auth_token_fallback_and_priority() {
    let tmp = tempfile::tempdir().unwrap();
    let key_token = "ANTHROPIC_AUTH_TOKEN";
    let key_api = "ANTHROPIC_API_KEY";
    let prev_token = std::env::var(key_token).ok();
    let prev_api = std::env::var(key_api).ok();

    // --- Test 1: AUTH_TOKEN used when API_KEY absent ---
    std::env::remove_var(key_api);
    std::env::set_var(key_token, "sk-test-auth-token-123");

    let settings = load_settings(Some(tmp.path().to_str().unwrap()));
    assert_eq!(
        settings.api_key.as_deref(),
        Some("sk-test-auth-token-123"),
        "AUTH_TOKEN should set api_key when API_KEY is absent"
    );

    // --- Test 2: API_KEY takes priority over AUTH_TOKEN ---
    std::env::set_var(key_api, "sk-api-key-priority");
    std::env::set_var(key_token, "sk-auth-token-fallback");

    let settings = load_settings(Some(tmp.path().to_str().unwrap()));
    assert_eq!(
        settings.api_key.as_deref(),
        Some("sk-api-key-priority"),
        "API_KEY should take priority over AUTH_TOKEN"
    );

    // Restore.
    match prev_token {
        Some(v) => std::env::set_var(key_token, v),
        None => std::env::remove_var(key_token),
    }
    match prev_api {
        Some(v) => std::env::set_var(key_api, v),
        None => std::env::remove_var(key_api),
    }
}

#[test]
fn test_env_override_model_aliases() {
    let tmp = tempfile::tempdir().unwrap();
    let keys = [
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
    ];
    let prev: Vec<_> = keys.iter().map(|k| (k, std::env::var(k).ok())).collect();

    std::env::set_var(keys[0], "claude-haiku-4.5-custom");
    std::env::set_var(keys[1], "claude-sonnet-4.6-custom");
    std::env::set_var(keys[2], "claude-opus-4.6-custom");

    let settings = load_settings(Some(tmp.path().to_str().unwrap()));
    assert_eq!(settings.default_haiku_model.as_deref(), Some("claude-haiku-4.5-custom"));
    assert_eq!(settings.default_sonnet_model.as_deref(), Some("claude-sonnet-4.6-custom"));
    assert_eq!(settings.default_opus_model.as_deref(), Some("claude-opus-4.6-custom"));

    // Restore.
    for (k, prev_val) in prev {
        match prev_val {
            Some(v) => std::env::set_var(k, v),
            None => std::env::remove_var(k),
        }
    }
}

#[test]
fn test_toml_then_env_override() {
    let tmp = tempfile::tempdir().unwrap();
    let toml = r#"
model = "claude-sonnet-4-20250514"
max_tokens = 4096
"#;
    fs::write(tmp.path().join("settings.toml"), toml).unwrap();

    let prev_model = std::env::var("OXICODE_MODEL").ok();
    std::env::set_var("OXICODE_MODEL", "claude-opus-4-20250514");

    let settings = load_settings(Some(tmp.path().to_str().unwrap()));
    // Env should override TOML.
    assert_eq!(settings.model, "claude-opus-4-20250514");
    // TOML value for max_tokens should remain.
    assert_eq!(settings.max_tokens, 4096);

    match prev_model {
        Some(v) => std::env::set_var("OXICODE_MODEL", v),
        None => std::env::remove_var("OXICODE_MODEL"),
    }
}

// ── Settings Defaults ───────────────────────────────────────────

#[test]
fn test_default_settings_values() {
    let s = Settings::default();
    assert!(s.api_key.is_none());
    assert!(!s.model.is_empty(), "Default model should be set");
    assert!(s.max_tokens > 0, "Default max_tokens should be positive");
    assert_eq!(s.permission_mode, "default");
    assert_eq!(s.editor_mode, "normal");
    assert!(s.default_haiku_model.is_none());
    assert!(s.default_sonnet_model.is_none());
    assert!(s.default_opus_model.is_none());
}

#[test]
fn test_settings_toml_roundtrip() {
    let mut settings = Settings::default();
    settings.model = "test-model".to_string();
    settings.max_tokens = 9999;
    settings.theme = "dark".to_string();

    let toml_str = toml::to_string_pretty(&settings).unwrap();
    let parsed: Settings = toml::from_str(&toml_str).unwrap();

    assert_eq!(parsed.model, "test-model");
    assert_eq!(parsed.max_tokens, 9999);
    assert_eq!(parsed.theme, "dark");
}

// ── CLAUDE.md Discovery ─────────────────────────────────────────

#[test]
fn test_discover_claude_md_in_project_root() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("CLAUDE.md"), "# Project Instructions\nBe helpful.").unwrap();

    let result = discover_claude_md(tmp.path());
    assert!(result.is_some(), "Should find CLAUDE.md in project root");
    let (path, content) = result.unwrap();
    assert!(path.ends_with("CLAUDE.md"));
    assert!(content.contains("Project Instructions"));
}

#[test]
fn test_discover_oxicode_md_takes_precedence() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("CLAUDE.md"), "claude content").unwrap();
    fs::write(tmp.path().join("OXICODE.md"), "oxicode content").unwrap();

    let result = discover_claude_md(tmp.path());
    assert!(result.is_some());
    let (path, content) = result.unwrap();
    assert!(
        path.ends_with("OXICODE.md"),
        "OXICODE.md should take precedence, got: {}",
        path.display()
    );
    assert_eq!(content, "oxicode content");
}

#[test]
fn test_discover_walks_up_to_git_root() {
    let tmp = tempfile::tempdir().unwrap();
    // Create .git directory at root (marks project boundary).
    fs::create_dir_all(tmp.path().join(".git")).unwrap();
    fs::write(tmp.path().join("CLAUDE.md"), "root instructions").unwrap();

    // Create nested subdirectory.
    let nested = tmp.path().join("src").join("utils");
    fs::create_dir_all(&nested).unwrap();

    let result = discover_claude_md(&nested);
    assert!(result.is_some(), "Should find CLAUDE.md by walking up to .git root");
    let (_, content) = result.unwrap();
    assert_eq!(content, "root instructions");
}

#[test]
fn test_discover_stops_at_git_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    // Create CLAUDE.md ABOVE the .git boundary.
    fs::write(tmp.path().join("CLAUDE.md"), "should not find this").unwrap();

    // Create a subdir with its own .git (marks it as a separate project).
    let subproject = tmp.path().join("subproject");
    fs::create_dir_all(subproject.join(".git")).unwrap();
    // No CLAUDE.md in subproject.

    let result = discover_claude_md(&subproject);
    assert!(
        result.is_none(),
        "Should NOT find CLAUDE.md above .git boundary"
    );
}

#[test]
fn test_discover_empty_dir_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let result = discover_claude_md(tmp.path());
    assert!(result.is_none(), "Empty dir should return None");
}
