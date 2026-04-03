//! OpenAI-compatible provider: works with OpenAI, DeepSeek, Ollama, OpenRouter, etc.
//!
//! Uses the OpenAI chat completions API with SSE streaming.
//! Converts Claude tool schemas to OpenAI function-calling format via schema_adapter.

use futures::StreamExt;
use oxicode_common::{OxiError, OxiResult, Usage};

use crate::provider::{EventStream, LlmProvider, MessageRequest};
use crate::proxy::build_proxy_client;
use crate::rate_limit_headers::parse_openai_headers;
use crate::retry::RetryPolicy;
use crate::schema_adapter::{
    claude_tools_to_openai_functions, openai_finish_reason_to_stop_reason,
};
use crate::stream_event::StreamEvent;

/// Provider for any OpenAI-compatible API endpoint.
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    api_key: Option<String>,
    base_url: String,
    provider_name: String,
    retry_policy: RetryPolicy,
    /// Extra headers to send with every request (e.g., OpenRouter's HTTP-Referer).
    extra_headers: Vec<(String, String)>,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        provider_name: impl Into<String>,
    ) -> Self {
        Self {
            client: build_proxy_client(),
            api_key,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            provider_name: provider_name.into(),
            retry_policy: RetryPolicy::default(),
            extra_headers: Vec::new(),
        }
    }

    /// Add extra headers (e.g., for OpenRouter).
    pub fn with_extra_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.extra_headers = headers;
        self
    }

    /// Build request body in OpenAI chat completions format.
    fn build_request_body(&self, request: &MessageRequest) -> serde_json::Value {
        let messages = self.convert_messages(request);

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens,
            "stream": request.stream,
        });

        if !request.tools.is_empty() {
            let functions = claude_tools_to_openai_functions(&request.tools);
            body["tools"] = serde_json::json!(functions);
        }

        body
    }

    /// Convert our Message format to OpenAI messages format.
    fn convert_messages(&self, request: &MessageRequest) -> Vec<serde_json::Value> {
        let mut messages = Vec::new();

        // System prompt as a system message.
        if let Some(system) = &request.system {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system,
            }));
        }

        for msg in &request.messages {
            match msg.role {
                oxicode_common::Role::System => {}
                oxicode_common::Role::User => {
                    // Check if this message contains tool results.
                    let has_tool_results = msg
                        .content
                        .iter()
                        .any(|b| matches!(b, oxicode_common::ContentBlock::ToolResult { .. }));

                    if has_tool_results {
                        // Emit each tool result as a separate "tool" role message.
                        for block in &msg.content {
                            if let oxicode_common::ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                ..
                            } = block
                            {
                                messages.push(serde_json::json!({
                                    "role": "tool",
                                    "tool_call_id": tool_use_id,
                                    "content": content,
                                }));
                            }
                        }
                    } else {
                        messages.push(serde_json::json!({
                            "role": "user",
                            "content": msg.text(),
                        }));
                    }
                }
                oxicode_common::Role::Assistant => {
                    let mut assistant_msg = serde_json::json!({
                        "role": "assistant",
                    });

                    // Collect text content.
                    let text: String = msg
                        .content
                        .iter()
                        .filter_map(|b| b.as_text())
                        .collect::<Vec<_>>()
                        .join("");

                    if !text.is_empty() {
                        assistant_msg["content"] = serde_json::json!(text);
                    }

                    // Collect tool uses as tool_calls.
                    let tool_calls: Vec<serde_json::Value> = msg
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            oxicode_common::ContentBlock::ToolUse { id, name, input } => {
                                Some(serde_json::json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": input.to_string(),
                                    }
                                }))
                            }
                            _ => None,
                        })
                        .collect();

                    if !tool_calls.is_empty() {
                        assistant_msg["tool_calls"] = serde_json::json!(tool_calls);
                    }

                    messages.push(assistant_msg);
                }
            }
        }

        messages
    }
}

/// Parse a single SSE data line from OpenAI streaming format.
/// Returns None for `[DONE]` sentinel.
fn parse_openai_sse_chunk(data: &str) -> Option<Vec<StreamEvent>> {
    if data.trim() == "[DONE]" {
        return None;
    }

    let chunk: serde_json::Value = serde_json::from_str(data).ok()?;
    let choices = chunk.get("choices")?.as_array()?;

    let mut events = Vec::new();

    // Usage info (some providers include it in the final chunk).
    if let Some(usage) = chunk.get("usage") {
        let input = usage
            .get("prompt_tokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32;
        let output = usage
            .get("completion_tokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32;
        if input > 0 || output > 0 {
            events.push(StreamEvent::UsageUpdate(Usage {
                input_tokens: input,
                output_tokens: output,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }));
        }
    }

    for choice in choices {
        let delta = choice.get("delta");
        let finish_reason = choice.get("finish_reason").and_then(|v| v.as_str());

        if let Some(delta) = delta {
            // Text content delta.
            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    events.push(StreamEvent::TextDelta {
                        text: content.to_string(),
                    });
                }
            }

            // Tool call deltas.
            if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                for tc in tool_calls {
                    // Tool call start: has id and function.name
                    if let (Some(id), Some(func)) =
                        (tc.get("id").and_then(|v| v.as_str()), tc.get("function"))
                    {
                        if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                            events.push(StreamEvent::ToolUseStart {
                                id: id.to_string(),
                                name: name.to_string(),
                            });
                        }
                    }

                    // Tool call argument delta.
                    if let Some(args) = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|v| v.as_str())
                    {
                        if !args.is_empty() {
                            events.push(StreamEvent::ToolInputDelta {
                                partial_json: args.to_string(),
                            });
                        }
                    }
                }
            }
        }

        // Finish reason.
        if let Some(reason) = finish_reason {
            let stop_reason = openai_finish_reason_to_stop_reason(reason);

            // Emit ContentBlockStop before MessageStop for tool calls.
            if reason == "tool_calls" {
                events.push(StreamEvent::ContentBlockStop { index: 0 });
            }

            events.push(StreamEvent::MessageStop { stop_reason });
        }
    }

    Some(events)
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    #[allow(clippy::too_many_lines)]
    async fn stream_message(&self, request: MessageRequest) -> OxiResult<EventStream> {
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();
        let extra_headers = self.extra_headers.clone();
        let body = self.build_request_body(&request);
        let body_str = serde_json::to_string(&body).map_err(|e| OxiError::api(e.to_string()))?;
        let retry_policy = self.retry_policy.clone();

        let stream = async_stream::stream! {
            let mut retry_count = 0u32;

            'retry: loop {
                let mut req_builder = client
                    .post(format!("{base_url}/chat/completions"))
                    .header("content-type", "application/json")
                    .body(body_str.clone());

                // Auth header.
                if let Some(key) = &api_key {
                    req_builder = req_builder.header("authorization", format!("Bearer {key}"));
                }

                // Extra headers (OpenRouter, etc.).
                for (k, v) in &extra_headers {
                    req_builder = req_builder.header(k.as_str(), v.as_str());
                }

                let response = match req_builder.send().await {
                    Ok(r) => r,
                    Err(e) => {
                        if retry_count < retry_policy.max_retries {
                            retry_count += 1;
                            let delay = retry_policy.delay_for(retry_count);
                            tracing::warn!("Request error (retry {}/{}): {} — retrying in {:?}",
                                retry_count, retry_policy.max_retries, e, delay);
                            tokio::time::sleep(delay).await;
                            continue 'retry;
                        }
                        yield Err(OxiError::api(format!("Request failed after {retry_count} retries: {e}")));
                        return;
                    }
                };

                if !response.status().is_success() {
                    let status = response.status();
                    if status.as_u16() == 429 && retry_count < retry_policy.max_retries {
                        retry_count += 1;
                        let info = parse_openai_headers(response.headers());
                        let delay = retry_policy.delay_for_rate_limit(retry_count, &info);
                        tracing::warn!(
                            "Rate limited (retry {}/{}): {} — retrying in {:?}",
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
                    let body_text = response.text().await.unwrap_or_default();
                    yield Err(OxiError::api(format!("API error {status}: {body_text}")));
                    return;
                }

                // Read SSE stream line by line.
                let mut byte_stream = response.bytes_stream();
                let mut buffer = String::new();

                while let Some(chunk_result) = byte_stream.next().await {
                    match chunk_result {
                        Ok(bytes) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));

                            // Process complete lines.
                            while let Some(newline_pos) = buffer.find('\n') {
                                let line = buffer[..newline_pos].trim().to_string();
                                buffer = buffer[newline_pos + 1..].to_string();

                                if line.is_empty() || line == ":" {
                                    continue;
                                }

                                if let Some(data) = line.strip_prefix("data: ") {
                                    match parse_openai_sse_chunk(data) {
                                        Some(events) => {
                                            let has_stop = events.iter().any(|e| {
                                                matches!(e, StreamEvent::MessageStop { .. })
                                            });
                                            for event in events {
                                                yield Ok(event);
                                            }
                                            if has_stop {
                                                return;
                                            }
                                        }
                                        None => {
                                            // [DONE] — stream finished.
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            if retry_count < retry_policy.max_retries {
                                retry_count += 1;
                                let delay = retry_policy.delay_for(retry_count);
                                tracing::warn!("Stream error (retry {}/{}): {} — retrying in {:?}",
                                    retry_count, retry_policy.max_retries, e, delay);
                                tokio::time::sleep(delay).await;
                                continue 'retry;
                            }
                            yield Err(OxiError::api(format!("Stream error: {e}")));
                            return;
                        }
                    }
                }

                // Stream ended without [DONE] — emit EndTurn.
                yield Ok(StreamEvent::MessageStop {
                    stop_reason: oxicode_common::StopReason::EndTurn,
                });
                return;
            }
        };

        Ok(Box::pin(stream))
    }

    fn name(&self) -> &str {
        &self.provider_name
    }
}

/// Create an OpenAI provider with standard config.
pub fn openai_provider(api_key: String) -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::new("https://api.openai.com/v1", Some(api_key), "openai")
}

/// Create an Ollama provider (local, no auth).
pub fn ollama_provider(base_url: Option<String>) -> OpenAiCompatibleProvider {
    let url = base_url.unwrap_or_else(|| "http://localhost:11434/v1".to_string());
    OpenAiCompatibleProvider::new(url, None, "ollama")
}

/// Create a DeepSeek provider.
pub fn deepseek_provider(api_key: String) -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::new("https://api.deepseek.com/v1", Some(api_key), "deepseek")
}

/// Create an Azure OpenAI provider.
/// `endpoint` should be like `https://YOUR-RESOURCE.openai.azure.com/openai/deployments/YOUR-DEPLOYMENT`
/// `api_version` defaults to "2024-06-01" if None.
pub fn azure_openai_provider(
    endpoint: String,
    api_key: String,
    api_version: Option<String>,
) -> OpenAiCompatibleProvider {
    let version = api_version.unwrap_or_else(|| "2024-06-01".to_string());
    let url = format!("{endpoint}?api-version={version}");
    let mut provider = OpenAiCompatibleProvider::new(url, None, "azure");
    provider.extra_headers = vec![("api-key".to_string(), api_key)];
    provider
}

/// Create an OpenRouter provider.
pub fn openrouter_provider(api_key: String) -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::new("https://openrouter.ai/api/v1", Some(api_key), "openrouter")
        .with_extra_headers(vec![
            (
                "HTTP-Referer".to_string(),
                "https://oxicode.dev".to_string(),
            ),
            ("X-Title".to_string(), "OxiCode".to_string()),
        ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_chunk() {
        let data = r#"{"id":"chatcmpl-1","choices":[{"index":0,"delta":{"content":"Hello"}}]}"#;
        let events = parse_openai_sse_chunk(data).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], StreamEvent::TextDelta { text } if text == "Hello"));
    }

    #[test]
    fn test_parse_done() {
        assert!(parse_openai_sse_chunk("[DONE]").is_none());
    }

    #[test]
    fn test_parse_tool_call_start() {
        let data = r#"{"choices":[{"delta":{"tool_calls":[{"id":"call_1","type":"function","function":{"name":"bash","arguments":""}}]}}]}"#;
        let events = parse_openai_sse_chunk(data).unwrap();
        assert!(events
            .iter()
            .any(|e| matches!(e, StreamEvent::ToolUseStart { name, .. } if name == "bash")));
    }

    #[test]
    fn test_parse_finish_reason_stop() {
        let data = r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#;
        let events = parse_openai_sse_chunk(data).unwrap();
        assert!(events.iter().any(|e| matches!(
            e,
            StreamEvent::MessageStop {
                stop_reason: oxicode_common::StopReason::EndTurn
            }
        )));
    }

    #[test]
    fn test_parse_usage_chunk() {
        let data = r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#;
        let events = parse_openai_sse_chunk(data).unwrap();
        assert!(events.iter().any(|e| matches!(
            e,
            StreamEvent::UsageUpdate(Usage {
                input_tokens: 10,
                output_tokens: 5,
                ..
            })
        )));
    }

    #[test]
    fn test_message_conversion_system_prompt() {
        let provider = openai_provider("test-key".to_string());
        let request = MessageRequest::new("gpt-4o", vec![oxicode_common::Message::user("hi")])
            .with_system("You are helpful.");
        let body = provider.build_request_body(&request);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are helpful.");
        assert_eq!(messages[1]["role"], "user");
    }
}
