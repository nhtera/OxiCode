//! Google Vertex AI provider for Claude models.
//!
//! Uses Google OAuth2 bearer token for authentication.
//! Streams via Vertex AI's `streamRawPredict` endpoint which returns standard SSE.

use futures::StreamExt;
use oxicode_common::constants::ANTHROPIC_API_VERSION;
use oxicode_common::{OxiError, OxiResult};
use reqwest_eventsource::{Event, EventSource};

use crate::provider::{EventStream, LlmProvider, MessageRequest};
use crate::proxy::build_proxy_client;
use crate::retry::RetryPolicy;
use crate::stream_event::{RawSseEvent, StreamEvent};

/// Google Vertex AI provider.
pub struct VertexProvider {
    client: reqwest::Client,
    project_id: String,
    region: String,
    access_token: String,
    retry_policy: RetryPolicy,
}

impl VertexProvider {
    pub fn new(project_id: String, region: String, access_token: String) -> Self {
        Self {
            client: build_proxy_client(),
            project_id,
            region,
            access_token,
            retry_policy: RetryPolicy::default(),
        }
    }

    /// Build Vertex AI endpoint URL for a given model.
    fn endpoint_url(&self, model: &str) -> String {
        format!(
            "https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/publishers/anthropic/models/{model}:streamRawPredict",
            region = self.region,
            project = self.project_id,
        )
    }

    /// Build the Anthropic-compatible request body for Vertex AI.
    fn build_request_body(&self, request: &MessageRequest) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| m.role != oxicode_common::Role::System)
            .map(|m| {
                let content: Vec<serde_json::Value> = m
                    .content
                    .iter()
                    .map(|block| match block {
                        oxicode_common::ContentBlock::Text { text } => {
                            serde_json::json!({"type": "text", "text": text})
                        }
                        oxicode_common::ContentBlock::Image { source } => {
                            serde_json::json!({
                                "type": "image",
                                "source": {
                                    "type": &source.source_type,
                                    "media_type": &source.media_type,
                                    "data": &source.data,
                                }
                            })
                        }
                        oxicode_common::ContentBlock::ToolUse { id, name, input } => {
                            serde_json::json!({
                                "type": "tool_use", "id": id, "name": name, "input": input,
                            })
                        }
                        oxicode_common::ContentBlock::ToolResult {
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
                        oxicode_common::ContentBlock::Thinking { thinking } => {
                            serde_json::json!({"type": "thinking", "thinking": thinking})
                        }
                    })
                    .collect();
                serde_json::json!({"role": m.role, "content": content})
            })
            .collect();

        let mut body = serde_json::json!({
            "anthropic_version": ANTHROPIC_API_VERSION,
            "model": request.model,
            "max_tokens": request.max_tokens,
            "messages": messages,
            "stream": true,
        });

        if let Some(system) = &request.system {
            body["system"] = serde_json::json!(system);
        }

        if !request.tools.is_empty() {
            body["tools"] = serde_json::json!(request.tools);
        }

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
impl LlmProvider for VertexProvider {
    async fn stream_message(&self, request: MessageRequest) -> OxiResult<EventStream> {
        let client = self.client.clone();
        let access_token = self.access_token.clone();
        let url = self.endpoint_url(&request.model);
        let body_str = serde_json::to_string(&self.build_request_body(&request))
            .map_err(|e| OxiError::api(e.to_string()))?;
        let retry_policy = self.retry_policy.clone();

        // Vertex AI returns standard SSE — same parsing as AnthropicProvider.
        let stream = async_stream::stream! {
            let mut retry_count = 0u32;

            'retry: loop {
                let req = client
                    .post(&url)
                    .header("authorization", format!("Bearer {access_token}"))
                    .header("content-type", "application/json")
                    .body(body_str.clone());

                let mut es = match EventSource::new(req) {
                    Ok(es) => es,
                    Err(e) => {
                        yield Err(OxiError::api(format!("Failed to create Vertex SSE stream: {e}")));
                        return;
                    }
                };

                loop {
                    match es.next().await {
                        Some(Ok(Event::Open)) => {
                            tracing::debug!("Vertex AI SSE connection opened");
                            retry_count = 0;
                        }
                        Some(Ok(Event::Message(msg))) => {
                            match serde_json::from_str::<RawSseEvent>(&msg.data) {
                                Ok(raw) => {
                                    let events = raw.into_stream_events();
                                    let has_stop = events.iter().any(|e| {
                                        matches!(e, StreamEvent::MessageStop { .. })
                                    });
                                    for event in events {
                                        yield Ok(event);
                                    }
                                    if has_stop {
                                        es.close();
                                        return;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to parse Vertex SSE: {e} — data: {}", msg.data);
                                }
                            }
                        }
                        Some(Err(e)) => {
                            es.close();
                            if retry_count < retry_policy.max_retries {
                                retry_count += 1;
                                let delay = retry_policy.delay_for(retry_count);
                                tracing::warn!("Vertex SSE error (retry {}/{}): {e}",
                                    retry_count, retry_policy.max_retries);
                                tokio::time::sleep(delay).await;
                                continue 'retry;
                            }
                            yield Err(OxiError::api(
                                format!("Vertex stream error after {retry_count} retries: {e}")
                            ));
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

    fn name(&self) -> &str {
        "vertex"
    }
}
