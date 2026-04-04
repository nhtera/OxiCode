use futures::StreamExt;
use oxicode_common::constants::{ANTHROPIC_API_URL, ANTHROPIC_API_VERSION};
use oxicode_common::{ContentBlock, OxiError, OxiResult, Role};
use reqwest_eventsource::{Event, EventSource};

use crate::provider::{EventStream, LlmProvider, MessageRequest};
use crate::proxy::build_proxy_client;
use crate::rate_limit_headers::parse_anthropic_headers;
use crate::retry::RetryPolicy;
use crate::stream_event::{RawSseEvent, StreamEvent};

/// Anthropic API provider for Claude models.
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    retry_policy: RetryPolicy,
    /// When true, use `Authorization: Bearer` instead of `x-api-key`.
    use_bearer_auth: bool,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: build_proxy_client(),
            api_key: api_key.into(),
            base_url: ANTHROPIC_API_URL.to_string(),
            retry_policy: RetryPolicy::default(),
            use_bearer_auth: false,
        }
    }

    /// Create a provider using OAuth Bearer token authentication.
    pub fn with_oauth_token(token: impl Into<String>) -> Self {
        Self {
            client: build_proxy_client(),
            api_key: token.into(),
            base_url: ANTHROPIC_API_URL.to_string(),
            retry_policy: RetryPolicy::default(),
            use_bearer_auth: true,
        }
    }

    /// Override base URL (for testing).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Build the `anthropic-beta` header value from request beta features.
    fn build_beta_header(request: &MessageRequest) -> Option<String> {
        if request.beta_features.is_empty() {
            return None;
        }
        Some(request.beta_features.join(","))
    }

    /// Build the JSON request body for the Anthropic Messages API.
    fn build_request_body(&self, request: &MessageRequest) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| {
                let content: Vec<serde_json::Value> = m
                    .content
                    .iter()
                    .map(|block| match block {
                        ContentBlock::Text { text } => {
                            serde_json::json!({"type": "text", "text": text})
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            serde_json::json!({
                                "type": "tool_use",
                                "id": id,
                                "name": name,
                                "input": input,
                            })
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => {
                            serde_json::json!({
                                "type": "tool_result",
                                "tool_use_id": tool_use_id,
                                "content": content,
                                "is_error": is_error,
                            })
                        }
                        ContentBlock::Thinking { thinking } => {
                            serde_json::json!({"type": "thinking", "thinking": thinking})
                        }
                    })
                    .collect();

                serde_json::json!({
                    "role": m.role,
                    "content": content,
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": request.model,
            "max_tokens": request.max_tokens,
            "messages": messages,
            "stream": request.stream,
        });

        // System prompt — with optional cache_control for prompt caching.
        if let Some(system) = &request.system {
            if request.prompt_caching {
                body["system"] = serde_json::json!([{
                    "type": "text",
                    "text": system,
                    "cache_control": {"type": "ephemeral"}
                }]);
            } else {
                body["system"] = serde_json::json!(system);
            }
        }

        // Tools — with optional cache_control on the last tool for prompt caching.
        if !request.tools.is_empty() {
            if request.prompt_caching {
                let mut tools = request.tools.clone();
                if let Some(last) = tools.last_mut() {
                    if let Some(obj) = last.as_object_mut() {
                        obj.insert(
                            "cache_control".to_string(),
                            serde_json::json!({"type": "ephemeral"}),
                        );
                    }
                }
                body["tools"] = serde_json::json!(tools);
            } else {
                body["tools"] = serde_json::json!(request.tools);
            }
        }

        // Extended thinking.
        if let Some(thinking) = &request.thinking {
            if thinking.enabled {
                body["thinking"] = serde_json::json!({
                    "type": "enabled",
                    "budget_tokens": thinking.budget_tokens
                });
            }
        }

        body
    }
}

#[async_trait::async_trait]
impl LlmProvider for AnthropicProvider {
    async fn stream_message(&self, request: MessageRequest) -> OxiResult<EventStream> {
        // Clone values needed inside the stream (avoid borrowing self).
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();
        let beta_header = Self::build_beta_header(&request);
        let body_str = serde_json::to_string(&self.build_request_body(&request))
            .map_err(|e| OxiError::api(e.to_string()))?;
        let retry_policy = self.retry_policy.clone();
        let use_bearer = self.use_bearer_auth;

        let stream = async_stream::stream! {
            let mut retry_count = 0u32;

            // C2 FIX: Outer retry loop reconstructs EventSource on each attempt.
            'retry: loop {
                let mut req = client
                    .post(format!("{base_url}/v1/messages"))
                    .header("anthropic-version", ANTHROPIC_API_VERSION)
                    .header("content-type", "application/json")
                    .body(body_str.clone());

                // Use Bearer auth for OAuth tokens, x-api-key for API keys.
                if use_bearer {
                    req = req.header("authorization", format!("Bearer {api_key}"));
                } else {
                    req = req.header("x-api-key", &api_key);
                }

                if let Some(beta) = &beta_header {
                    req = req.header("anthropic-beta", beta.as_str());
                }

                let mut es = match EventSource::new(req) {
                    Ok(es) => es,
                    Err(e) => {
                        yield Err(OxiError::api(format!("Failed to create SSE stream: {e}")));
                        return;
                    }
                };

                loop {
                    match es.next().await {
                        Some(Ok(Event::Open)) => {
                            tracing::debug!("SSE connection opened");
                            retry_count = 0;
                        }
                        Some(Ok(Event::Message(msg))) => {
                            match serde_json::from_str::<RawSseEvent>(&msg.data) {
                                Ok(raw) => {
                                    let events = raw.into_stream_events();
                                    let has_stop = events.iter().any(|e| matches!(e, StreamEvent::MessageStop { .. }));
                                    for event in events {
                                        yield Ok(event);
                                    }
                                    if has_stop {
                                        es.close();
                                        return;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to parse SSE: {} — data: {}", e, msg.data);
                                }
                            }
                        }
                        Some(Err(e)) => {
                            es.close();
                            // Check if this is a 429 rate limit error with headers.
                            if let reqwest_eventsource::Error::InvalidStatusCode(status, ref response) = e {
                                if status.as_u16() == 429 && retry_count < retry_policy.max_retries {
                                    retry_count += 1;
                                    let info = parse_anthropic_headers(response.headers());
                                    let delay = retry_policy.delay_for_rate_limit(retry_count, &info);
                                    tracing::warn!(
                                        "Anthropic rate limited (retry {}/{}): {} — retrying in {:?}",
                                        retry_count, retry_policy.max_retries, info.message, delay
                                    );
                                    yield Ok(StreamEvent::RateLimited {
                                        info,
                                        attempt: retry_count,
                                        max_retries: retry_policy.max_retries,
                                        retry_in_secs: delay.as_secs_f64(),
                                    });
                                    tokio::time::sleep(delay).await;
                                    continue 'retry;
                                }
                            }
                            if retry_count < retry_policy.max_retries {
                                retry_count += 1;
                                let delay = retry_policy.delay_for(retry_count);
                                tracing::warn!("SSE error (retry {}/{}): {} — reconnecting in {:?}",
                                    retry_count, retry_policy.max_retries, e, delay);
                                tokio::time::sleep(delay).await;
                                continue 'retry;
                            }
                            yield Err(OxiError::api(format!("SSE stream error after {retry_count} retries: {e}")));
                            return;
                        }
                        None => {
                            es.close();
                            return;
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    fn name(&self) -> &'static str {
        "anthropic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxicode_common::Message;

    fn make_provider() -> AnthropicProvider {
        AnthropicProvider::new("test-key")
    }

    #[test]
    fn test_prompt_caching_system_prompt() {
        let provider = make_provider();
        let request = MessageRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")])
            .with_system("You are helpful.")
            .with_prompt_caching(true);

        let body = provider.build_request_body(&request);

        // System should be an array with cache_control.
        let system = body["system"].as_array().expect("system should be array");
        assert_eq!(system.len(), 1);
        assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(system[0]["text"], "You are helpful.");
    }

    #[test]
    fn test_prompt_caching_tools() {
        let provider = make_provider();
        let mut request =
            MessageRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")])
                .with_prompt_caching(true);
        request.tools = vec![
            serde_json::json!({"name": "tool1", "description": "first"}),
            serde_json::json!({"name": "tool2", "description": "second"}),
        ];

        let body = provider.build_request_body(&request);
        let tools = body["tools"].as_array().expect("tools should be array");

        // Only the last tool should have cache_control.
        assert!(tools[0].get("cache_control").is_none());
        assert_eq!(tools[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_no_caching_when_disabled() {
        let provider = make_provider();
        let request = MessageRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")])
            .with_system("You are helpful.");

        let body = provider.build_request_body(&request);

        // System should be a plain string, not an array.
        assert!(body["system"].is_string());
    }

    #[test]
    fn test_extended_thinking_in_body() {
        let provider = make_provider();
        let request = MessageRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")])
            .with_thinking(10000);

        let body = provider.build_request_body(&request);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 10000);
    }

    #[test]
    fn test_thinking_budget_clamped_to_minimum() {
        let request = MessageRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")])
            .with_thinking(500);

        // Should be clamped to 1024 minimum.
        assert_eq!(request.thinking.unwrap().budget_tokens, 1024);
    }

    #[test]
    fn test_no_thinking_by_default() {
        let provider = make_provider();
        let request = MessageRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")]);

        let body = provider.build_request_body(&request);
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn test_beta_header_with_caching() {
        let request = MessageRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")])
            .with_prompt_caching(true);

        let beta = AnthropicProvider::build_beta_header(&request);
        assert_eq!(beta, Some("prompt-caching-2024-07-31".to_string()));
    }

    #[test]
    fn test_beta_header_empty_when_no_features() {
        let request = MessageRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")]);

        let beta = AnthropicProvider::build_beta_header(&request);
        assert!(beta.is_none());
    }

    #[test]
    fn test_beta_header_multiple_features() {
        let mut request =
            MessageRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")])
                .with_prompt_caching(true);
        request
            .beta_features
            .push("max-tokens-3-5-sonnet-2024-07-15".to_string());

        let beta = AnthropicProvider::build_beta_header(&request).unwrap();
        assert!(beta.contains("prompt-caching-2024-07-31"));
        assert!(beta.contains("max-tokens-3-5-sonnet-2024-07-15"));
    }
}
