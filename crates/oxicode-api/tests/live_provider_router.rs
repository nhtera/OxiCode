//! Integration tests for the provider router and model alias resolution.
//!
//! Tests env var fallback chain, model alias resolution, and provider detection.
//! The `test_from_env_with_auth_token` test requires live API credentials.
//!
//! Run with: `cargo test -p oxicode-api --test live_provider_router`

use oxicode_api::ProviderRouter;

// ── Model Alias Resolution ──────────────────────────────────────

#[test]
fn test_resolve_alias_sonnet_with_env() {
    // Temporarily set env var for this test.
    let key = "ANTHROPIC_DEFAULT_SONNET_MODEL";
    let prev = std::env::var(key).ok();
    std::env::set_var(key, "claude-sonnet-4.6");

    let router = ProviderRouter::from_env();
    let result = router.resolve("sonnet");
    assert!(result.is_ok(), "Should resolve 'sonnet'");
    assert_eq!(
        result.unwrap().model,
        "claude-sonnet-4.6",
        "Should resolve to env var value"
    );

    // Restore.
    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

#[test]
fn test_resolve_alias_haiku_with_env() {
    let key = "ANTHROPIC_DEFAULT_HAIKU_MODEL";
    let prev = std::env::var(key).ok();
    std::env::set_var(key, "claude-haiku-4.5-test");

    let router = ProviderRouter::from_env();
    let result = router.resolve("haiku");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().model, "claude-haiku-4.5-test");

    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

#[test]
fn test_resolve_alias_opus_with_env() {
    let key = "ANTHROPIC_DEFAULT_OPUS_MODEL";
    let prev = std::env::var(key).ok();
    std::env::set_var(key, "claude-opus-4.6-test");

    let router = ProviderRouter::from_env();
    let result = router.resolve("opus");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().model, "claude-opus-4.6-test");

    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

#[test]
fn test_resolve_full_model_name_passthrough() {
    let router = ProviderRouter::from_env();
    let result = router.resolve("claude-sonnet-4-20250514");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().model, "claude-sonnet-4-20250514");
}

#[test]
fn test_resolve_non_claude_model_passthrough() {
    let router = ProviderRouter::from_env();
    let result = router.resolve("gpt-4o");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().model, "gpt-4o");
}

// ── Provider Detection ──────────────────────────────────────────

#[test]
fn test_router_has_anthropic_when_auth_token_set() {
    // If ANTHROPIC_AUTH_TOKEN or ANTHROPIC_API_KEY is set, anthropic should be available.
    let has_key = std::env::var("ANTHROPIC_API_KEY").is_ok()
        || std::env::var("ANTHROPIC_AUTH_TOKEN").is_ok();

    let router = ProviderRouter::from_env();
    let providers = router.available_providers();

    if has_key {
        assert!(
            providers.contains(&"anthropic"),
            "Should have anthropic provider when credentials set. Available: {providers:?}"
        );
    }
    // Ollama is always available (no key needed).
    assert!(
        providers.contains(&"ollama"),
        "Should always have ollama provider. Available: {providers:?}"
    );
}

#[test]
fn test_router_explicit_provider_prefix() {
    let router = ProviderRouter::from_env();
    // "ollama:llama3" should route to ollama provider with model "llama3"
    let result = router.resolve("ollama:llama3");
    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.model, "llama3");
}

// ── Live API test (requires credentials) ────────────────────────

#[tokio::test]
#[ignore]
async fn test_from_env_with_auth_token() {
    // Requires ANTHROPIC_AUTH_TOKEN + ANTHROPIC_BASE_URL env vars.
    let token = std::env::var("ANTHROPIC_AUTH_TOKEN")
        .expect("ANTHROPIC_AUTH_TOKEN required for this test");
    assert!(!token.is_empty(), "Token should not be empty");

    let router = ProviderRouter::from_env_with_oauth(None);
    let providers = router.available_providers();
    assert!(
        providers.contains(&"anthropic"),
        "Should detect anthropic from AUTH_TOKEN"
    );

    // Resolve a model and verify the provider works.
    let resolved = router.resolve("sonnet").expect("Should resolve 'sonnet'");
    assert!(
        !resolved.model.is_empty(),
        "Resolved model should not be empty"
    );
}
