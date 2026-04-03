# Provider Integration Guide

**Version:** 1.0 | **Date:** 2026-04-03 | **Scope:** LLM provider implementation

## Overview

This guide explains how to implement custom LLM providers and use provider features in OxiCode.

---

## MessageRequest Builder Pattern

All request configuration uses fluent builders:

```rust
let request = MessageRequest::new("claude-3-5-sonnet", messages)
    .with_system("system prompt")
    .with_max_tokens(4096)
    .with_prompt_caching(true)        // Anthropic only
    .with_thinking(10_000);            // Anthropic only
```

**API:**
```rust
impl MessageRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self
    pub fn with_system(mut self, system: impl Into<String>) -> Self
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self
    pub fn with_prompt_caching(mut self, enabled: bool) -> Self
    pub fn with_thinking(mut self, budget_tokens: u32) -> Self
}
```

**Rules:**
1. Always use builders for configuration (never direct field assignment)
2. New fields default to disabled/empty (no-op on other providers)
3. Builders return `Self` for chaining
4. Provider implementations safely ignore unsupported features

---

## Provider Implementation Pattern

**File Location:** `crates/oxicode-api/src/{provider_name}.rs`

### Trait Implementation

All providers must implement `LlmProvider`:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stream a message response as SSE events.
    async fn stream_message(&self, request: MessageRequest) -> OxiResult<EventStream>;
    
    /// Provider name for logging/routing.
    fn name(&self) -> &str;
}
```

### Example Implementation

```rust
use async_trait::async_trait;
use crate::provider::{EventStream, LlmProvider, MessageRequest};
use crate::stream_event::StreamEvent;
use oxicode_common::OxiResult;

pub struct CustomProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl CustomProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: "https://api.custom.com".to_string(),
        }
    }
    
    fn build_request_body(&self, request: &MessageRequest) -> serde_json::Value {
        // Normalize to Anthropic format for compatibility
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| m.role != oxicode_common::Role::System)
            .map(|m| {
                let content = m.content.iter().map(|block| {
                    match block {
                        oxicode_common::ContentBlock::Text { text } => {
                            serde_json::json!({"type": "text", "text": text})
                        }
                        // Handle other block types...
                        _ => serde_json::json!({}),
                    }
                }).collect::<Vec<_>>();
                
                serde_json::json!({
                    "role": m.role,
                    "content": content,
                })
            })
            .collect();
        
        serde_json::json!({
            "model": request.model,
            "max_tokens": request.max_tokens,
            "messages": messages,
            "stream": request.stream,
        })
    }
    
    fn build_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "Authorization",
            format!("Bearer {}", self.api_key)
                .parse()
                .expect("valid header"),
        );
        headers.insert(
            "Content-Type",
            "application/json".parse().expect("valid header"),
        );
        headers
    }
}

#[async_trait]
impl LlmProvider for CustomProvider {
    async fn stream_message(&self, request: MessageRequest) -> OxiResult<EventStream> {
        debug!("custom: streaming {} with model {}", self.name(), request.model);
        
        let body = self.build_request_body(&request);
        let url = format!("{}/v1/messages", self.base_url);
        
        let response = self.client
            .post(&url)
            .headers(self.build_headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| OxiError::Api {
                message: format!("request failed: {}", e),
                status: None,
                retryable: true,
            })?;
        
        // Parse SSE stream and convert to StreamEvent
        // Implementation depends on response format
        // See anthropic.rs for SSE example
        
        Ok(Box::pin(stream_events))
    }
    
    fn name(&self) -> &str {
        "custom"
    }
}
```

### Key Requirements

**All providers must:**
1. Implement `LlmProvider` trait with `stream_message()` method
2. Return `OxiResult<EventStream>` (boxed async stream of events)
3. Build request body compatible with Anthropic message format
4. Parse responses into standard `StreamEvent` types
5. Log provider name and model in debug logs
6. Use `tracing` for all logging (not `println!`)
7. Handle authentication errors distinctly from other errors
8. Support streaming (do not buffer entire response)

---

## Environment Variable Conventions

**Provider env vars follow pattern:**
- `{PROVIDER}_API_KEY` — Secret API key
- `{PROVIDER}_ENDPOINT` — Alternate endpoint URL
- `{PROVIDER}_REGION` — Region/location (with sensible defaults)

**Examples:**

```bash
# Anthropic
ANTHROPIC_API_KEY=sk-ant-...

# OpenAI
OPENAI_API_KEY=sk-proj-...

# AWS Bedrock (SigV4 auth)
AWS_ACCESS_KEY_ID=AKIA...
AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI...
AWS_BEDROCK_REGION=us-east-1              # Optional

# Google Vertex AI (OAuth2)
VERTEX_AI_PROJECT=my-gcp-project
VERTEX_AI_ACCESS_TOKEN=ya29.a0A...        # From gcloud auth
VERTEX_AI_REGION=us-central1               # Optional
```

**Rule:** ProviderRouter auto-detects from env vars; no manual setup needed in code.

---

## Provider Registration

### In ProviderRouter

File: `crates/oxicode-api/src/provider_router.rs`

Register new providers in `ProviderRouter::from_env()`:

```rust
impl ProviderRouter {
    pub fn from_env() -> Self {
        let mut providers = Vec::new();
        
        // Anthropic (highest priority)
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            providers.push(("anthropic".to_string(), Arc::new(AnthropicProvider::new(key))));
        }
        
        // Custom provider
        if let Ok(key) = std::env::var("CUSTOM_API_KEY") {
            providers.push(("custom".to_string(), Arc::new(CustomProvider::new(key))));
        }
        
        ProviderRouter { providers }
    }
    
    pub fn resolve(&self, model: &str) -> OxiResult<ResolvedProvider> {
        // Resolve model name to provider
        // Use prefixes (custom::, bedrock::, vertex::) for explicit routing
    }
}
```

### In lib.rs

File: `crates/oxicode-api/src/lib.rs`

Export provider:

```rust
pub mod custom;

pub use custom::CustomProvider;
```

---

## Testing Providers

### Unit Tests

Test request body building and header construction:

```rust
#[test]
fn test_build_request_body_with_system_prompt() {
    let provider = CustomProvider::new("test-key");
    let request = MessageRequest::new("test-model", vec![])
        .with_system("system prompt");
    
    let body = provider.build_request_body(&request);
    
    assert_eq!(body["model"], "test-model");
    assert!(body["system"].is_array());
}

#[test]
fn test_headers_include_auth() {
    let provider = CustomProvider::new("secret-key");
    let headers = provider.build_headers();
    
    assert!(headers.get("Authorization").is_some());
    assert!(headers.get("Authorization").unwrap().to_str().unwrap().contains("secret-key"));
}
```

### Integration Tests

Test with mock HTTP server (use `mockito` crate):

```rust
#[tokio::test]
async fn test_stream_message_success() {
    let _m = mockito::mock("POST", mockito::Matcher::Any)
        .with_status(200)
        .with_body("data: {\"type\": \"content_block_start\"}\n\n")
        .create();
    
    let provider = CustomProvider::new("test-key")
        .with_base_url(mockito::server_url());
    
    let request = MessageRequest::new("test-model", vec![]);
    let stream = provider.stream_message(request).await;
    
    assert!(stream.is_ok());
}

#[tokio::test]
async fn test_auth_error() {
    let _m = mockito::mock("POST", mockito::Matcher::Any)
        .with_status(401)
        .with_body(r#"{"error": "unauthorized"}"#)
        .create();
    
    let provider = CustomProvider::new("bad-key")
        .with_base_url(mockito::server_url());
    
    let request = MessageRequest::new("test-model", vec![]);
    let stream = provider.stream_message(request).await;
    
    assert!(stream.is_err());
}
```

---

## Debugging Providers

**Enable debug logging:**

```bash
RUST_LOG=debug cargo run
# or specific to oxicode-api:
RUST_LOG=oxicode_api=debug cargo run
```

**Key log points:**

```rust
debug!("custom: streaming {} with model {}", self.name(), request.model);
debug!("custom: request headers: {:?}", headers);
debug!("custom: response status: {}", response.status());
```

**Common issues:**

| Issue | Debug Steps |
|-------|------------|
| 401 Unauthorized | Check API key env var, validate format |
| 429 Rate Limited | Add retry policy, implement backoff |
| Timeout | Check network, increase timeout duration |
| Malformed response | Log raw response body before parsing |
| Empty stream | Verify `stream: true` in request body |

---

## Feature Flags by Provider

| Provider | Prompt Caching | Extended Thinking | Beta Headers |
|----------|----------------|-------------------|--------------|
| Anthropic | ✓ | ✓ | ✓ |
| OpenAI | ✗ | ✗ | ✗ |
| Bedrock | ✓ | ✓ | ✓ |
| Vertex AI | ✓ | ✓ | ✓ |
| Custom | depends | depends | depends |

---

## References

- `phase-2-api-enhancement.md` — Detailed provider specifications
- `crates/oxicode-api/src/anthropic.rs` — Reference implementation
- `crates/oxicode-api/src/bedrock/mod.rs` — Advanced example (SigV4)
- `crates/oxicode-api/src/vertex.rs` — Example with OAuth2
