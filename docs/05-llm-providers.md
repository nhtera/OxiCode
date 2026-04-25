# LLM Providers Design

**Version:** 1.0  
**Last Updated:** 2026-04-12  
**Related:** `crates/oxicode-api/src/`, `crates/oxicode-core/src/query_engine.rs`

## Overview

OxiCode abstracts LLM providers behind a common trait, enabling seamless switching between Claude (Anthropic), OpenAI, DeepSeek, Ollama, AWS Bedrock, Google Vertex, and custom endpoints. All providers implement streaming responses using the same event protocol.

**Design Goals:**
- **Unified Interface:** Single `LlmProvider` trait for all backends
- **Streaming First:** All responses are SSE (Server-Sent Events) streams
- **Provider Agnostic:** Query engine doesn't care which provider is active
- **Resilient:** Built-in retry logic, rate-limit handling, and timeout recovery

---

## LlmProvider Trait

### Definition

```rust
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stream a message response as SSE events.
    async fn stream_message(&self, request: MessageRequest) -> OxiResult<EventStream>;

    /// Provider name for logging/routing.
    fn name(&self) -> &str;
}
```

### EventStream Type Alias

```rust
pub type EventStream = Pin<Box<dyn Stream<Item = OxiResult<StreamEvent>> + Send>>;
```

A boxed async stream yielding `StreamEvent` items or errors.

(source: `crates/oxicode-api/src/provider.rs`)

---

## MessageRequest Struct

Complete request specification sent to any LLM provider.

### Definition

```rust
pub struct MessageRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub system: Option<String>,
    pub max_tokens: u32,
    pub stream: bool,
    pub tools: Vec<serde_json::Value>,
    /// Enable prompt caching (Anthropic-specific).
    pub prompt_caching: bool,
    /// Extended thinking configuration.
    pub thinking: Option<ThinkingConfig>,
    /// Beta features to request (e.g., "prompt-caching-2024-07-31").
    pub beta_features: Vec<String>,
}

pub struct ThinkingConfig {
    pub enabled: bool,
    pub budget_tokens: u32,
}
```

### Builder Methods

```rust
impl MessageRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self { }
    pub fn with_system(self, system: impl Into<String>) -> Self { }
    pub fn with_max_tokens(self, max_tokens: u32) -> Self { }
    pub fn with_prompt_caching(self, enabled: bool) -> Self { }
    pub fn with_thinking(self, budget_tokens: u32) -> Self { }
}
```

See [LlmProvider Trait](#llmprovidertrait) for builder example.

(source: `crates/oxicode-api/src/provider.rs`)

---

## StreamEvent Protocol

### Complete Enum

```rust
pub enum StreamEvent {
    /// A chunk of text content.
    TextDelta { text: String },

    /// A chunk of thinking content (extended thinking).
    ThinkingDelta { thinking: String },

    /// Model started a tool use block.
    ToolUseStart { id: String, name: String },

    /// Partial JSON input for a tool use.
    ToolInputDelta { partial_json: String },

    /// Content block completed.
    ContentBlockStop { index: u32 },

    /// Token usage update.
    UsageUpdate(Usage),

    /// Message completed.
    MessageStop { stop_reason: StopReason },

    /// Rate limited — provider returned 429, retry in progress.
    RateLimited {
        info: RateLimitInfo,
        attempt: u32,
        max_retries: u32,
        retry_in_secs: f64,
    },

    /// Prompt cache break detected — Anthropic cache invalidated.
    CacheBreakDetected(CacheBreakEvent),

    /// Non-rate-limit retry in progress (e.g., 502, connection error).
    Retrying {
        message: String,
        attempt: u32,
        max_retries: u32,
        retry_in_secs: f64,
    },

    /// Stream error.
    Error { message: String },

    /// Ping / keep-alive (ignored by consumers).
    Ping,
}
```

### StopReason Enum

```rust
pub enum StopReason {
    EndTurn,      // Model finished naturally
    ToolUse,      // Model called a tool
    MaxTokens,    // Hit max_tokens limit
    StopSequence, // Stop sequence triggered
}
```

### Usage Struct

```rust
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_creation_input_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
}
```

### RateLimitInfo Struct

```rust
pub struct RateLimitInfo {
    pub retry_after_secs: Option<f64>,
    pub limit_type: RateLimitType,
    pub remaining: Option<u32>,
    pub message: String,
    // ... other fields
}

pub enum RateLimitType {
    TokensPerMinute,
    RequestsPerMinute,
    TokensPerDay,
    InputTokensPerMinute,
    OutputTokensPerMinute,
}
```

(source: `crates/oxicode-api/src/stream_event.rs`)

---

## StreamEvent Sequence Diagram

### Typical Response Sequence

```
┌─────────────────────────────────────────────────────┐
│ message_start (UsageUpdate with initial tokens)      │
├─────────────────────────────────────────────────────┤
│ content_block_start (TextDelta or ToolUseStart)      │
├─────────────────────────────────────────────────────┤
│ content_block_delta (TextDelta, ToolInputDelta, etc) │
│ content_block_delta (more deltas...)                 │
│ content_block_delta (...)                            │
├─────────────────────────────────────────────────────┤
│ content_block_stop                                   │
├─────────────────────────────────────────────────────┤
│ message_delta (UsageUpdate + MessageStop)            │
└─────────────────────────────────────────────────────┘
```

### Tool Use Sequence

```
message_start
content_block_start { id: "call_123", name: "bash" }
ToolInputDelta { partial_json: "{\"com" }
ToolInputDelta { partial_json: "mand\": \"echo" }
ToolInputDelta { partial_json: " hello\"}" }
content_block_stop
message_stop { stop_reason: ToolUse }
```

### Extended Thinking Sequence

```
message_start
content_block_start (thinking)
ThinkingDelta { "Let me analyze..." }
ThinkingDelta { "First step..." }
content_block_stop
content_block_start (text)
TextDelta { "Based on my analysis..." }
TextDelta { " here's the answer." }
content_block_stop
message_stop { stop_reason: EndTurn }
```

### Mapping to CoreEvent (TUI)

```rust
StreamEvent::TextDelta { text }
    → CoreEvent::TextDelta(text)

StreamEvent::ThinkingDelta { thinking }
    → CoreEvent::ThinkingDelta(thinking)

StreamEvent::ToolUseStart { id, name }
    → CoreEvent::ToolUseStart { id, name, input: "..." }
    (input accumulates as ToolInputDeltas arrive)

StreamEvent::ToolInputDelta { partial_json }
    → (accumulates into input)

StreamEvent::MessageStop { .. }
    → CoreEvent::StreamEnd

StreamEvent::RateLimited { info, attempt, max_retries, retry_in_secs }
    → CoreEvent::RateLimited { message, attempt, max_retries, retry_in_secs }

StreamEvent::Error { message }
    → CoreEvent::Error(message)
```

(source: `crates/oxicode-tui/src/events.rs`)

---

## Provider Implementations

### 1. Anthropic Provider

**Direct API access to Claude models via `api.anthropic.com`**

#### Struct

```rust
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    retry_policy: RetryPolicy,
    use_bearer_auth: bool,
}
```

#### Features
- **Prompt Caching:** `with_prompt_caching(true)` — adds `cache_control: {"type": "ephemeral"}` to system & tools
- **Extended Thinking:** `with_thinking(budget_tokens)` — sets `thinking: {"type": "enabled", "budget_tokens": N}`
- **Beta Features:** Auto-sets `anthropic-beta` header for new APIs
- **OAuth Support:** `with_oauth_token(token)` uses Bearer auth instead of API key
- **Custom Base URL:** `with_base_url(url)` for API proxies/testing

#### Streaming Implementation
- SSE-based streaming via `reqwest_eventsource`
- **Stream read timeout:** 90 seconds (catches stalled connections)
- **Outer retry loop:** Reconstructs EventSource on each retry attempt
- **Rate limit handling:** Parses `retry-after-ms`, `anthropic-ratelimit-*` headers

#### Builder Pattern

```rust
let provider = AnthropicProvider::new("sk-ant-...");
// or
let provider = AnthropicProvider::with_oauth_token("oauth-token");
// or (used by ProviderRouter — auto-selects auth header)
let provider = AnthropicProvider::with_token_auto_detect("sk-ant-... or gateway-token");
```

#### Auth Header Auto-Detection (`with_token_auto_detect`)

`ProviderRouter` uses `with_token_auto_detect` for both `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN`. The auth header is chosen as follows:

| Condition | Header sent |
|-----------|-------------|
| `OXICODE_AUTH_HEADER=bearer` | `Authorization: Bearer …` |
| `OXICODE_AUTH_HEADER=x-api-key` | `x-api-key: …` |
| Token starts with `sk-ant-` | `x-api-key: …` (genuine Anthropic key) |
| Any other token prefix | `Authorization: Bearer …` (assume gateway/proxy) |

**Escape hatch:** Set `OXICODE_AUTH_HEADER=bearer` or `OXICODE_AUTH_HEADER=x-api-key` to force a specific scheme regardless of token format. This is useful when a custom token format starts with `sk-ant-` but targets a non-Anthropic gateway.

(source: `crates/oxicode-api/src/anthropic.rs`, `crates/oxicode-common/src/constants.rs`)

---

### 2. OpenAI-Compatible Provider

**Works with OpenAI, DeepSeek, Ollama, OpenRouter, Azure, etc.**

#### Struct

```rust
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    api_key: Option<String>,
    base_url: String,
    provider_name: String,
    retry_policy: RetryPolicy,
    extra_headers: Vec<(String, String)>,
}
```

#### Features
- **Schema Conversion:** Automatically converts Claude tool format to OpenAI `functions`
- **Message Format:** Handles OpenAI's tool result format (separate `tool` role messages)
- **Extra Headers:** Supports custom headers (e.g., OpenRouter's `HTTP-Referer`)
- **Optional API Key:** Works with local (Ollama) or authenticated endpoints
- **Fallback Support:** Has built-in providers for OpenAI, DeepSeek, Ollama, Azure

#### Supported Providers

```rust
// OpenAI
pub fn openai_provider(api_key: String) -> OpenAiCompatibleProvider { }

// DeepSeek
pub fn deepseek_provider(api_key: String) -> OpenAiCompatibleProvider { }

// Ollama (local, no auth)
pub fn ollama_provider() -> OpenAiCompatibleProvider { }

// OpenRouter
pub fn openrouter_provider(api_key: String) -> OpenAiCompatibleProvider { }

// Azure OpenAI
pub fn azure_openai_provider(endpoint: String, api_key: String, version: Option<String>)
    -> OpenAiCompatibleProvider { }
```

#### Schema Adapter

```rust
fn claude_tool_to_openai_function(tool: &Value) -> Value {
    // Converts: {"name": "bash", "description": "...", "input_schema": {...}}
    //      to: {"type": "function", "function": {"name": "bash", "parameters": {...}}}
}
```

(source: `crates/oxicode-api/src/openai_compatible.rs`)

---

### 3. AWS Bedrock Provider

**AWS-hosted Claude models with SigV4 request signing**

#### Struct

```rust
pub struct BedrockProvider {
    client: reqwest::Client,
    access_key: String,
    secret_key: String,
    session_token: Option<String>,
    region: String,
    retry_policy: RetryPolicy,
}
```

#### Features
- **SigV4 Signing:** Automatically signs requests with AWS credentials
- **Session Tokens:** Supports temporary credentials (STS)
- **Region Routing:** Allows cross-region model access
- **Anthropic Format:** Uses Anthropic Messages API format (`anthropic_version: "bedrock-2023-05-31"`)
- **Event-Stream Binary Format:** Parses AWS event-stream (binary framing)

#### Endpoint Format

```
POST https://{region}.bedrock-runtime.amazonaws.com/model/{model-id}/converse-stream
```

#### Authentication Flow

```
1. Load AWS credentials (access_key, secret_key, optional session_token)
2. Build Anthropic-format request body
3. Sign request with SigV4 (AWS Signature Version 4)
4. POST to Bedrock endpoint
5. Parse event-stream binary format (AWS-specific framing)
6. Convert to StreamEvent sequence
```

(source: `crates/oxicode-api/src/bedrock/mod.rs`)

---

### 4. Google Vertex AI Provider

**Google Cloud Vertex AI with OAuth Bearer token**

#### Struct

```rust
pub struct VertexProvider {
    client: reqwest::Client,
    project_id: String,
    region: String,
    access_token: String,
    retry_policy: RetryPolicy,
}
```

#### Features
- **OAuth Bearer Token:** Uses Google OAuth2 access tokens
- **Project & Region:** Multi-project support with region routing
- **Anthropic Format:** Uses Anthropic Messages API format
- **Endpoint Routing:** Constructs region-specific URLs

#### Endpoint Format

```
POST https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/publishers/anthropic/models/{model}:streamRawPredict
```

#### Authentication

```
Bearer {access_token}  (Google OAuth2)
```

(source: `crates/oxicode-api/src/vertex.rs`)

---

### 5. Mock Provider (Testing)

**Pre-configured response sequences for integration testing**

#### Struct

```rust
pub struct MockLlmProvider {
    responses: Arc<Vec<Vec<StreamEvent>>>,
    call_index: Arc<AtomicUsize>,
    provider_name: String,
}
```

#### Usage

```rust
// Simple text response
let mock = MockLlmProvider::with_text("Hello, world!");

// Tool use + text response
let mock = MockLlmProvider::with_tool_then_text(
    "call_1",
    "bash",
    serde_json::json!({"command": "ls -la"}),
    "Here are the results:"
);

// Custom sequence
let events = vec![
    StreamEvent::TextDelta { text: "Processing...".to_string() },
    StreamEvent::TextDelta { text: " done!".to_string() },
    StreamEvent::MessageStop { stop_reason: StopReason::EndTurn },
];
let mock = MockLlmProvider::new(vec![events]);
```

(source: `crates/oxicode-api/src/mock.rs`)

---

### 6. Proxy Provider (Custom Endpoints)

**Forward requests to arbitrary HTTP endpoints**

#### Struct

```rust
pub struct ProxyProvider {
    base_url: String,
    api_key: Option<String>,
}
```

#### Use Cases
- Internal LLM services
- Local dev servers
- Anthropic API proxies with additional middleware

(source: `crates/oxicode-api/src/proxy.rs`)

---

## Provider Router

### Purpose
Resolves model identifiers to the correct LLM provider and handles runtime switching.

### Struct

```rust
pub struct ProviderRouter {
    providers: Vec<(String, Arc<dyn LlmProvider>)>,
}

pub struct ResolvedProvider {
    pub provider: Arc<dyn LlmProvider>,
    pub model: String,
}
```

### Automatic Provider Detection

```rust
let router = ProviderRouter::from_env();
```

Detects available providers from environment variables (in order):

```
1. ANTHROPIC_API_KEY        → AnthropicProvider
2. ANTHROPIC_AUTH_TOKEN     → AnthropicProvider (fallback)
3. ANTHROPIC_BASE_URL       → Custom Anthropic endpoint
4. OPENAI_API_KEY           → OpenAI provider
5. DEEPSEEK_API_KEY         → DeepSeek provider
6. OPENROUTER_API_KEY       → OpenRouter provider
7. AZURE_OPENAI_ENDPOINT    → Azure OpenAI provider
8. AWS_ACCESS_KEY_ID        → AWS Bedrock provider
9. GOOGLE_APPLICATION_CRED  → Google Vertex provider
10. OLLAMA_BASE_URL         → Ollama (local)
```

### OAuth Support

```rust
let router = ProviderRouter::from_env_with_oauth(Some("oauth-token"));
```

OAuth token takes priority over API keys for Anthropic.

### Model Resolution

```rust
router.resolve_provider("claude-3-5-sonnet-20241022")?
    → ResolvedProvider { provider, model }

router.resolve_provider("gpt-4")?
    → ResolvedProvider { provider, model }

router.resolve_provider("ollama:llama2")?
    → ResolvedProvider { provider, model }
```

(source: `crates/oxicode-api/src/provider_router.rs`)

---

## Retry & Error Handling

### RetryPolicy Struct

```rust
pub struct RetryPolicy {
    pub max_retries: u32,           // Default: 3
    pub base_delay: Duration,       // Default: 1s
    pub max_delay: Duration,        // Default: 120s
}
```

### Exponential Backoff with Jitter

```rust
impl RetryPolicy {
    /// Base delay (no jitter): 2^(attempt-1) * base_delay
    pub fn base_delay_for(&self, attempt: u32) -> Duration { }

    /// Delay with decorrelated jitter: base * (0.5 + random * 0.5)
    pub fn delay_for(&self, attempt: u32) -> Duration { }

    /// Respect rate-limit retry-after header
    pub fn delay_for_rate_limit(&self, attempt: u32, info: &RateLimitInfo) -> Duration {
        backoff.max(retry_after).min(max_delay)
    }

    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt <= max_retries
    }
}
```

### Default Schedule

```
Attempt 1: 0.5s - 1.0s
Attempt 2: 1.0s - 2.0s
Attempt 3: 2.0s - 4.0s
(capped at 120s)
```

### Retryable Errors
- **429 Rate Limited:** Uses `retry-after-ms` header if available
- **502/503/504:** Transient server errors
- **Connection timeouts:** Network hiccups
- **Stream stalls:** No data for 90 seconds

### Fatal Errors (No Retry)
- **401 Unauthorized:** Invalid credentials
- **403 Forbidden:** Access denied
- **404 Not Found:** Invalid model/endpoint
- **Invalid JSON:** Malformed response

(source: `crates/oxicode-api/src/retry.rs`)

---

## Rate Limit Handling

Parses provider-specific headers (Anthropic's `anthropic-ratelimit-*`, OpenAI's `x-ratelimit-*`) into `RateLimitInfo` struct. When rate limited, yields `StreamEvent::RateLimited`, waits with backoff, then retries. TUI displays: "Rate limited (tokens/min). Retrying in 30s... (1/3)".

(source: `crates/oxicode-api/src/rate_limit_headers.rs`)

---

## Schema Adapter

Claude → OpenAI function conversion: Maps Claude tool format `{name, description, input_schema}` to OpenAI format `{type: "function", function: {name, description, parameters}}`. Maps OpenAI finish reasons (`tool_calls`, `length`) to `StopReason` variants.

(source: `crates/oxicode-api/src/schema_adapter.rs`)

---

## Adding a New Provider

1. Create module in `crates/oxicode-api/src/my_provider.rs` with struct holding client, api_key, base_url, retry_policy
2. Implement `LlmProvider` trait: `async fn stream_message(&self, request: MessageRequest) -> OxiResult<EventStream>` using `async_stream!` and `reqwest` client
3. Build request body from `MessageRequest`, post to provider endpoint, parse SSE stream, yield `StreamEvent`
4. Register in `ProviderRouter::from_env()` by detecting env var (e.g., `MYPROVIDER_API_KEY`)
5. Export from `lib.rs` and add to public API
6. Write unit tests using `MockLlmProvider` for testing
7. Set env var and test: `export MYPROVIDER_API_KEY="..."; oxicode /model my-model-name`

(source: multiple provider impls)

## Data Flow: Message → Events → UI

```
┌─────────────────────────────────────────────────────────────┐
│ QueryEngine receives user input                              │
├─────────────────────────────────────────────────────────────┤
│ 1. Builds MessageRequest with system prompt, messages, tools │
├─────────────────────────────────────────────────────────────┤
│ 2. Selects provider from ProviderRouter based on model name  │
├─────────────────────────────────────────────────────────────┤
│ 3. Calls provider.stream_message(request)                    │
├─────────────────────────────────────────────────────────────┤
│ 4. Provider:                                                  │
│    - Converts MessageRequest to provider's format             │
│    - Sends HTTP POST with auth headers                       │
│    - Receives SSE stream                                     │
│    - Converts raw events to StreamEvent                      │
│    - Yields StreamEvent through async stream                 │
├─────────────────────────────────────────────────────────────┤
│ 5. QueryEngine consumes StreamEvent stream:                  │
│    - Accumulates tool inputs                                 │
│    - Dispatches tool calls via ToolRegistry                  │
│    - Checks permissions via PermissionPipeline               │
├─────────────────────────────────────────────────────────────┤
│ 6. TUI receives CoreEvent (mapped from StreamEvent)          │
│    - Renders text deltas in real-time                        │
│    - Shows thinking blocks                                   │
│    - Displays tool execution status                          │
└─────────────────────────────────────────────────────────────┘
```

---

## Integration with Query Engine

### Query Engine Responsibilities

1. **Create MessageRequest** — Assemble messages, system prompt, tools
2. **Select Provider** — Use ProviderRouter to get provider for model
3. **Stream Events** — Call `provider.stream_message(request)`
4. **Handle StreamEvents:**
   - `TextDelta` → Accumulate text, send to TUI
   - `ToolUseStart` → Create pending tool call
   - `ToolInputDelta` → Accumulate JSON input
   - `MessageStop` → Complete conversation turn
   - `RateLimited` → Emit CoreEvent, wait, retry
   - `Error` → Handle gracefully, retry or fail

### Tool Result Integration

After tool execution:

```rust
// 1. Tool completes with result
let result = tool.execute(input, ctx).await?;

// 2. Create ToolResult content block
let tool_result = ContentBlock::ToolResult {
    tool_use_id,
    content: result.output,
    is_error: result.is_error,
};

// 3. Add to messages for next LLM turn
messages.push(Message {
    role: Role::User,
    content: vec![tool_result],
});

// 4. Continue streaming with updated messages
let next_stream = provider.stream_message(
    MessageRequest::new(model, messages).with_tools(tools)
).await?;
```

(source: `crates/oxicode-core/src/query_engine.rs`)

---

## Testing Providers

Use `MockLlmProvider` to mock responses without hitting real APIs:
- `MockLlmProvider::with_text("Hello")` — simple text response
- `MockLlmProvider::with_tool_then_text(id, name, input_json, text)` — tool + response
- `MockLlmProvider::new(vec![events])` — custom event sequence

Enables testing rate limits, extended thinking, tool use, and error scenarios.

(source: `crates/oxicode-api/src/mock.rs`)

## Related Documentation

- **Query Engine:** `docs/02-query-engine.md`
- **Tool System:** `docs/03-tool-system.md`
- **Permission System:** `docs/04-permission-system.md`
- **System Architecture:** `docs/system-architecture.md`

---

## Unresolved Questions

- Should prompt caching be auto-enabled for all providers that support it?
- How should we handle providers with different tool schema formats (e.g., JSON Schema vs. TypeScript)?
- Should the router support weighted provider fallback (e.g., try Provider A, then Provider B)?
- How do we test streaming without mocking (integration tests with real APIs)?
