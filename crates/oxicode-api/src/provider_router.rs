//! Provider router: resolves model names to LLM providers.
//!
//! Auto-detects provider from model name prefix or env vars.
//! Supports runtime switching via the /model command.

use std::sync::Arc;

use oxicode_common::OxiResult;

use crate::bedrock::BedrockProvider;
use crate::openai_compatible::{
    azure_openai_provider, deepseek_provider, ollama_provider, openai_provider, openrouter_provider,
};
use crate::provider::LlmProvider;
use crate::vertex::VertexProvider;
use crate::AnthropicProvider;

/// Resolved provider with its target model name.
pub struct ResolvedProvider {
    pub provider: Arc<dyn LlmProvider>,
    pub model: String,
}

/// Routes model identifiers to the correct LLM provider.
pub struct ProviderRouter {
    /// Cached providers by name.
    providers: Vec<(String, Arc<dyn LlmProvider>)>,
}

impl ProviderRouter {
    /// Build a router by detecting available providers from environment variables.
    pub fn from_env() -> Self {
        Self::from_env_with_oauth(None)
    }

    /// Build a router with optional OAuth token for Anthropic.
    ///
    /// If `oauth_token` is provided, it takes priority over `ANTHROPIC_API_KEY`.
    /// Resolution order: OAuth token > `ANTHROPIC_API_KEY` > `ANTHROPIC_AUTH_TOKEN`.
    /// If `ANTHROPIC_BASE_URL` is set, the Anthropic provider uses that base URL.
    pub fn from_env_with_oauth(oauth_token: Option<String>) -> Self {
        let mut providers: Vec<(String, Arc<dyn LlmProvider>)> = Vec::new();

        // Anthropic: OAuth token > API key > Auth token.
        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty());

        if let Some(token) = oauth_token {
            let mut p = AnthropicProvider::with_oauth_token(token);
            if let Some(ref url) = base_url {
                p = p.with_base_url(url);
            }
            providers.push(("anthropic".to_string(), Arc::new(p)));
        } else if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            let mut p = AnthropicProvider::with_token_auto_detect(key);
            if let Some(ref url) = base_url {
                p = p.with_base_url(url);
            }
            providers.push(("anthropic".to_string(), Arc::new(p)));
        } else if let Ok(token) = std::env::var("ANTHROPIC_AUTH_TOKEN") {
            if !token.is_empty() {
                let mut p = AnthropicProvider::with_token_auto_detect(token);
                if let Some(ref url) = base_url {
                    p = p.with_base_url(url);
                }
                providers.push(("anthropic".to_string(), Arc::new(p)));
            }
        }

        // OpenAI.
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            providers.push(("openai".to_string(), Arc::new(openai_provider(key))));
        }

        // DeepSeek.
        if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
            providers.push(("deepseek".to_string(), Arc::new(deepseek_provider(key))));
        }

        // OpenRouter.
        if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
            providers.push(("openrouter".to_string(), Arc::new(openrouter_provider(key))));
        }

        // Azure OpenAI.
        if let (Ok(endpoint), Ok(key)) = (
            std::env::var("AZURE_OPENAI_ENDPOINT"),
            std::env::var("AZURE_OPENAI_API_KEY"),
        ) {
            let version = std::env::var("AZURE_OPENAI_API_VERSION").ok();
            providers.push((
                "azure".to_string(),
                Arc::new(azure_openai_provider(endpoint, key, version)),
            ));
        }

        // AWS Bedrock.
        if let (Ok(access_key), Ok(secret_key)) = (
            std::env::var("AWS_ACCESS_KEY_ID"),
            std::env::var("AWS_SECRET_ACCESS_KEY"),
        ) {
            let region = std::env::var("AWS_BEDROCK_REGION")
                .or_else(|_| std::env::var("AWS_REGION"))
                .unwrap_or_else(|_| "us-east-1".to_string());
            providers.push((
                "bedrock".to_string(),
                Arc::new(BedrockProvider::new(access_key, secret_key, region)),
            ));
        }

        // Google Vertex AI.
        if let (Ok(project), Ok(token)) = (
            std::env::var("VERTEX_AI_PROJECT").or_else(|_| std::env::var("GOOGLE_CLOUD_PROJECT")),
            std::env::var("VERTEX_AI_ACCESS_TOKEN"),
        ) {
            let region =
                std::env::var("VERTEX_AI_REGION").unwrap_or_else(|_| "us-central1".to_string());
            providers.push((
                "vertex".to_string(),
                Arc::new(VertexProvider::new(project, region, token)),
            ));
        }

        // Ollama (local, always available — no key needed).
        let ollama_url = std::env::var("OLLAMA_BASE_URL").ok();
        providers.push(("ollama".to_string(), Arc::new(ollama_provider(ollama_url))));

        Self { providers }
    }

    /// Resolve a model string to a provider.
    ///
    /// Resolution order:
    /// 1. Model alias from env var (e.g., "sonnet" → `ANTHROPIC_DEFAULT_SONNET_MODEL`)
    /// 2. Explicit prefix: `openai:gpt-4o` → OpenAI provider, model "gpt-4o"
    /// 3. Known model name patterns (claude-*, gpt-*, deepseek-*, etc.)
    /// 4. First available provider
    pub fn resolve(&self, model: &str) -> OxiResult<ResolvedProvider> {
        // 0. Resolve model alias from env vars.
        let model = resolve_model_alias(model);

        // 1. Explicit prefix: "provider:model"
        if let Some((prefix, model_name)) = model.split_once(':') {
            if let Some((_, provider)) = self.providers.iter().find(|(name, _)| name == prefix) {
                return Ok(ResolvedProvider {
                    provider: Arc::clone(provider),
                    model: model_name.to_string(),
                });
            }
        }

        // 2. Auto-detect from model name patterns.
        let detected = detect_provider_from_model(&model);
        if let Some(provider_name) = detected {
            if let Some((_, provider)) = self
                .providers
                .iter()
                .find(|(name, _)| name == &provider_name)
            {
                return Ok(ResolvedProvider {
                    provider: Arc::clone(provider),
                    model: model.clone(),
                });
            }
        }

        // 3. Fallback to first available provider.
        if let Some((_, provider)) = self.providers.first() {
            return Ok(ResolvedProvider {
                provider: Arc::clone(provider),
                model: model.clone(),
            });
        }

        Err(oxicode_common::OxiError::config(
            "No LLM providers configured. Set ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN, OPENAI_API_KEY, or another provider key.",
        ))
    }

    /// List available provider names.
    pub fn available_providers(&self) -> Vec<&str> {
        self.providers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Get a provider by name.
    pub fn get_provider(&self, name: &str) -> Option<Arc<dyn LlmProvider>> {
        self.providers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| Arc::clone(p))
    }
}

/// Resolve model aliases from environment variables.
///
/// Maps shorthand names to full model IDs via `ANTHROPIC_DEFAULT_*_MODEL` env vars.
/// Returns the original string if no alias matches.
fn resolve_model_alias(model: &str) -> String {
    let m = model.to_lowercase();
    let alias_env = match m.as_str() {
        "haiku" | "claude-haiku" => Some("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
        "sonnet" | "claude-sonnet" => Some("ANTHROPIC_DEFAULT_SONNET_MODEL"),
        "opus" | "claude-opus" => Some("ANTHROPIC_DEFAULT_OPUS_MODEL"),
        _ => None,
    };
    if let Some(env_key) = alias_env {
        if let Ok(val) = std::env::var(env_key) {
            if !val.is_empty() {
                return val;
            }
        }
    }
    model.to_string()
}

/// Detect provider from well-known model name prefixes/patterns.
fn detect_provider_from_model(model: &str) -> Option<String> {
    let m = model.to_lowercase();

    if m.starts_with("anthropic.claude") {
        return Some("bedrock".to_string());
    }
    if m.starts_with("claude-") || m.starts_with("anthropic/") {
        return Some("anthropic".to_string());
    }
    if m.starts_with("gpt-") || m.starts_with("o1-") || m.starts_with("o3-") || m.starts_with("o4-")
    {
        return Some("openai".to_string());
    }
    if m.starts_with("deepseek-") || m.starts_with("deepseek/") {
        return Some("deepseek".to_string());
    }
    if m.contains('/') {
        // Models with org/name format are likely OpenRouter.
        return Some("openrouter".to_string());
    }
    if m.starts_with("llama")
        || m.starts_with("mistral")
        || m.starts_with("phi")
        || m.starts_with("qwen")
        || m.starts_with("gemma")
    {
        return Some("ollama".to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_claude() {
        assert_eq!(
            detect_provider_from_model("claude-sonnet-4-20250514"),
            Some("anthropic".to_string())
        );
    }

    #[test]
    fn test_detect_gpt() {
        assert_eq!(
            detect_provider_from_model("gpt-4o"),
            Some("openai".to_string())
        );
    }

    #[test]
    fn test_detect_deepseek() {
        assert_eq!(
            detect_provider_from_model("deepseek-chat"),
            Some("deepseek".to_string())
        );
    }

    #[test]
    fn test_detect_ollama_patterns() {
        assert_eq!(
            detect_provider_from_model("llama3.1:8b"),
            Some("ollama".to_string())
        );
        assert_eq!(
            detect_provider_from_model("mistral-7b"),
            Some("ollama".to_string())
        );
    }

    #[test]
    fn test_detect_openrouter_slash_format() {
        assert_eq!(
            detect_provider_from_model("meta-llama/llama-3-70b"),
            Some("openrouter".to_string())
        );
    }

    #[test]
    fn test_detect_bedrock() {
        assert_eq!(
            detect_provider_from_model("anthropic.claude-3-sonnet-20240229-v1:0"),
            Some("bedrock".to_string())
        );
        assert_eq!(
            detect_provider_from_model("anthropic.claude-v2"),
            Some("bedrock".to_string())
        );
    }

    #[test]
    fn test_explicit_prefix() {
        // This tests the parsing logic, not the actual resolution.
        let (prefix, model) = "openai:gpt-4o".split_once(':').unwrap();
        assert_eq!(prefix, "openai");
        assert_eq!(model, "gpt-4o");
    }

    #[test]
    fn test_explicit_vertex_prefix() {
        let (prefix, model) = "vertex:claude-sonnet-4-20250514".split_once(':').unwrap();
        assert_eq!(prefix, "vertex");
        assert_eq!(model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_explicit_bedrock_prefix() {
        let (prefix, model) = "bedrock:anthropic.claude-3-sonnet".split_once(':').unwrap();
        assert_eq!(prefix, "bedrock");
        assert_eq!(model, "anthropic.claude-3-sonnet");
    }

    #[test]
    fn test_resolve_model_alias_sonnet() {
        // If env var not set, returns original.
        let result = resolve_model_alias("sonnet");
        // May or may not have env var; just test it doesn't panic.
        assert!(!result.is_empty());
    }

    #[test]
    fn test_resolve_model_alias_passthrough() {
        // Full model names pass through unchanged.
        let result = resolve_model_alias("claude-sonnet-4-20250514");
        assert_eq!(result, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_resolve_model_alias_unknown() {
        let result = resolve_model_alias("gpt-4o");
        assert_eq!(result, "gpt-4o");
    }
}
