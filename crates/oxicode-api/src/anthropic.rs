use futures::StreamExt;
use oxicode_common::constants::{ANTHROPIC_API_URL, ANTHROPIC_API_VERSION};
use oxicode_common::{ContentBlock, OxiError, OxiResult, Role};
use reqwest_eventsource::{Event, EventSource};

use crate::provider::{EventStream, LlmProvider, MessageRequest};
use crate::retry::RetryPolicy;
use crate::stream_event::{RawSseEvent, StreamEvent};

/// Anthropic API provider for Claude models.
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    retry_policy: RetryPolicy,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: ANTHROPIC_API_URL.to_string(),
            retry_policy: RetryPolicy::default(),
        }
    }

    /// Override base URL (for testing).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
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

        if let Some(system) = &request.system {
            body["system"] = serde_json::json!(system);
        }

        if !request.tools.is_empty() {
            body["tools"] = serde_json::json!(request.tools);
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
        let body_str = serde_json::to_string(&self.build_request_body(&request))
            .map_err(|e| OxiError::api(e.to_string()))?;
        let retry_policy = self.retry_policy.clone();

        let stream = async_stream::stream! {
            let mut retry_count = 0u32;

            // C2 FIX: Outer retry loop reconstructs EventSource on each attempt.
            'retry: loop {
                let req = client
                    .post(format!("{base_url}/v1/messages"))
                    .header("x-api-key", &api_key)
                    .header("anthropic-version", ANTHROPIC_API_VERSION)
                    .header("content-type", "application/json")
                    .body(body_str.clone());

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
                            if retry_count < retry_policy.max_retries {
                                retry_count += 1;
                                let delay = retry_policy.delay_for(retry_count);
                                tracing::warn!("SSE error (retry {}/{}): {} — reconnecting in {:?}",
                                    retry_count, retry_policy.max_retries, e, delay);
                                tokio::time::sleep(delay).await;
                                continue 'retry; // Reconstruct EventSource
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
