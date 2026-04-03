//! AWS Bedrock provider for Claude models.
//!
//! Uses SigV4 signing to authenticate with Bedrock Runtime.
//! Streams via `invoke-model-with-response-stream` and parses AWS event-stream binary format.

use futures::StreamExt;
use oxicode_common::{OxiError, OxiResult};

use crate::provider::{EventStream, LlmProvider, MessageRequest};
use crate::retry::RetryPolicy;
use crate::stream_event::{RawSseEvent, StreamEvent};

mod sigv4;

/// AWS Bedrock provider.
pub struct BedrockProvider {
    client: reqwest::Client,
    access_key: String,
    secret_key: String,
    session_token: Option<String>,
    region: String,
    retry_policy: RetryPolicy,
}

impl BedrockProvider {
    pub fn new(access_key: String, secret_key: String, region: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            access_key,
            secret_key,
            session_token: std::env::var("AWS_SESSION_TOKEN").ok(),
            region,
            retry_policy: RetryPolicy::default(),
        }
    }

    /// Build the Anthropic-compatible request body for Bedrock.
    fn build_request_body(&self, request: &MessageRequest) -> serde_json::Value {
        // Bedrock uses the same body format as the Anthropic Messages API
        // but without the model field (model is in the URL).
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
            "anthropic_version": "bedrock-2023-05-31",
            "max_tokens": request.max_tokens,
            "messages": messages,
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

    /// Map a Claude model ID to a Bedrock model ID.
    fn bedrock_model_id(model: &str) -> String {
        // Bedrock uses anthropic.claude-* format
        if model.starts_with("anthropic.") {
            model.to_string()
        } else {
            format!("anthropic.{model}")
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for BedrockProvider {
    #[allow(clippy::too_many_lines)]
    async fn stream_message(&self, request: MessageRequest) -> OxiResult<EventStream> {
        let client = self.client.clone();
        let region = self.region.clone();
        let access_key = self.access_key.clone();
        let secret_key = self.secret_key.clone();
        let session_token = self.session_token.clone();
        let model_id = Self::bedrock_model_id(&request.model);
        let body_bytes = serde_json::to_vec(&self.build_request_body(&request))
            .map_err(|e| OxiError::api(e.to_string()))?;
        let retry_policy = self.retry_policy.clone();

        let stream = async_stream::stream! {
            let mut retry_count = 0u32;

            'retry: loop {
                let host = format!("bedrock-runtime.{region}.amazonaws.com");
                let uri = format!(
                    "https://{host}/model/{}/invoke-with-response-stream",
                    urlencoding::encode(&model_id)
                );

                // Sign the request with SigV4.
                let signed_headers = sigv4::sign_request(&sigv4::SignParams {
                    url: &uri,
                    host: &host,
                    region: &region,
                    service: "bedrock",
                    access_key: &access_key,
                    secret_key: &secret_key,
                    session_token: session_token.as_deref(),
                    body: &body_bytes,
                });

                let mut req = client
                    .post(&uri)
                    .header("content-type", "application/json")
                    .body(body_bytes.clone());

                for (key, value) in &signed_headers {
                    req = req.header(key.as_str(), value.as_str());
                }

                let response = match req.send().await {
                    Ok(r) => r,
                    Err(e) => {
                        if retry_count < retry_policy.max_retries {
                            retry_count += 1;
                            let delay = retry_policy.delay_for(retry_count);
                            tracing::warn!("Bedrock request error (retry {}/{}): {e}",
                                retry_count, retry_policy.max_retries);
                            tokio::time::sleep(delay).await;
                            continue 'retry;
                        }
                        yield Err(OxiError::api(format!("Bedrock request failed: {e}")));
                        return;
                    }
                };

                if !response.status().is_success() {
                    let status = response.status();
                    let body_text = response.text().await.unwrap_or_default();
                    if (status.as_u16() == 429 || status.as_u16() == 503)
                        && retry_count < retry_policy.max_retries
                    {
                        retry_count += 1;
                        let delay = retry_policy.delay_for(retry_count);
                        tracing::warn!("Bedrock throttled (retry {}/{}): {status}",
                            retry_count, retry_policy.max_retries);
                        tokio::time::sleep(delay).await;
                        continue 'retry;
                    }
                    yield Err(OxiError::api(format!("Bedrock error {status}: {body_text}")));
                    return;
                }

                // Parse AWS event-stream binary format.
                let mut byte_stream = response.bytes_stream();
                let mut buf = Vec::new();

                while let Some(chunk_result) = byte_stream.next().await {
                    match chunk_result {
                        Ok(bytes) => {
                            buf.extend_from_slice(&bytes);

                            // Process complete event-stream messages from buffer.
                            while let Some((payload, consumed)) = parse_event_stream_message(&buf) {
                                buf.drain(..consumed);

                                if let Some(json_str) = extract_payload_json(&payload) {
                                    match serde_json::from_str::<BedrockStreamChunk>(&json_str) {
                                        Ok(chunk) => {
                                            if let Some(bytes_data) = &chunk.bytes {
                                                // Decode the base64 payload which contains Anthropic SSE JSON.
                                                match base64_decode(bytes_data) {
                                                    Ok(decoded) => {
                                                        match serde_json::from_str::<RawSseEvent>(&decoded) {
                                                            Ok(raw) => {
                                                                let events = raw.into_stream_events();
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
                                                            Err(e) => {
                                                                tracing::warn!("Failed to parse Bedrock SSE JSON: {e}");
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!("Failed to decode Bedrock base64: {e}");
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!("Failed to parse Bedrock chunk: {e}");
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            if retry_count < retry_policy.max_retries {
                                retry_count += 1;
                                let delay = retry_policy.delay_for(retry_count);
                                tracing::warn!("Bedrock stream error (retry {}/{}): {e}",
                                    retry_count, retry_policy.max_retries);
                                tokio::time::sleep(delay).await;
                                continue 'retry;
                            }
                            yield Err(OxiError::api(format!("Bedrock stream error: {e}")));
                            return;
                        }
                    }
                }

                return;
            }
        };

        Ok(Box::pin(stream))
    }

    fn name(&self) -> &str {
        "bedrock"
    }
}

/// Bedrock wraps Anthropic events in a JSON envelope with base64-encoded bytes.
#[derive(serde::Deserialize)]
struct BedrockStreamChunk {
    bytes: Option<String>,
}

/// Simple base64 decoder (no padding required).
fn base64_decode(input: &str) -> Result<String, OxiError> {
    // Standard base64 alphabet decode
    let table: [u8; 256] = {
        let mut t = [255u8; 256];
        for (i, &c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
            .iter()
            .enumerate()
        {
            t[c as usize] = i as u8;
        }
        t
    };

    let input = input.trim_end_matches('=');
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;

    for &b in input.as_bytes() {
        let val = table[b as usize];
        if val == 255 {
            continue; // skip whitespace/invalid
        }
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }

    String::from_utf8(out).map_err(|e| OxiError::api(format!("Invalid UTF-8 in base64: {e}")))
}

/// Parse a single AWS event-stream binary message from the buffer.
/// Returns (payload_bytes, bytes_consumed) or None if incomplete.
fn parse_event_stream_message(buf: &[u8]) -> Option<(Vec<u8>, usize)> {
    // AWS event-stream format:
    // [4 bytes: total_length] [4 bytes: headers_length] [4 bytes: prelude_crc]
    // [headers...] [payload...] [4 bytes: message_crc]
    if buf.len() < 12 {
        return None;
    }

    let total_len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let headers_len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;

    if buf.len() < total_len {
        return None; // incomplete message
    }

    // Payload starts after prelude (12 bytes) + headers
    let payload_start = 12 + headers_len;
    // Payload ends 4 bytes before total (message CRC)
    let payload_end = total_len.saturating_sub(4);

    if payload_start > payload_end || payload_end > buf.len() {
        return Some((Vec::new(), total_len)); // empty/malformed, skip
    }

    let payload = buf[payload_start..payload_end].to_vec();
    Some((payload, total_len))
}

/// Extract JSON string from event-stream payload.
fn extract_payload_json(payload: &[u8]) -> Option<String> {
    if payload.is_empty() {
        return None;
    }
    String::from_utf8(payload.to_vec()).ok()
}

/// URL-encode a model ID for Bedrock endpoint paths.
mod urlencoding {
    use std::fmt::Write;

    pub fn encode(input: &str) -> String {
        let mut encoded = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    encoded.push(byte as char);
                }
                _ => {
                    let _ = write!(encoded, "%{byte:02X}");
                }
            }
        }
        encoded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxicode_common::Message;

    #[test]
    fn test_bedrock_model_id_passthrough() {
        assert_eq!(
            BedrockProvider::bedrock_model_id("anthropic.claude-3-sonnet"),
            "anthropic.claude-3-sonnet"
        );
    }

    #[test]
    fn test_bedrock_model_id_prefix_added() {
        assert_eq!(
            BedrockProvider::bedrock_model_id("claude-sonnet-4-20250514"),
            "anthropic.claude-sonnet-4-20250514"
        );
    }

    #[test]
    fn test_bedrock_request_body_has_anthropic_version() {
        let provider = BedrockProvider::new(
            "AKID".to_string(),
            "SECRET".to_string(),
            "us-east-1".to_string(),
        );
        let request = MessageRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")]);
        let body = provider.build_request_body(&request);

        assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");
        // Model should NOT be in body (it's in the URL for Bedrock).
        assert!(body.get("model").is_none());
    }

    #[test]
    fn test_bedrock_request_body_with_thinking() {
        let provider = BedrockProvider::new(
            "AKID".to_string(),
            "SECRET".to_string(),
            "us-east-1".to_string(),
        );
        let request = MessageRequest::new("claude-sonnet-4-20250514", vec![Message::user("hi")])
            .with_thinking(8000);
        let body = provider.build_request_body(&request);

        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 8000);
    }

    #[test]
    fn test_parse_event_stream_incomplete() {
        // Less than 12 bytes = incomplete.
        assert!(parse_event_stream_message(&[0, 0, 0, 20]).is_none());
    }

    #[test]
    fn test_parse_event_stream_valid_message() {
        // Build a minimal event-stream message:
        // total_len=24, headers_len=0, prelude_crc=0, payload="test1234", message_crc=0
        let mut msg = Vec::new();
        msg.extend_from_slice(&24u32.to_be_bytes()); // total_length
        msg.extend_from_slice(&0u32.to_be_bytes()); // headers_length
        msg.extend_from_slice(&0u32.to_be_bytes()); // prelude_crc
        msg.extend_from_slice(b"test1234"); // payload (8 bytes)
        msg.extend_from_slice(&0u32.to_be_bytes()); // message_crc

        let result = parse_event_stream_message(&msg);
        assert!(result.is_some());
        let (payload, consumed) = result.unwrap();
        assert_eq!(consumed, 24);
        assert_eq!(payload, b"test1234");
    }

    #[test]
    fn test_base64_decode_simple() {
        assert_eq!(base64_decode("SGVsbG8=").unwrap(), "Hello");
        assert_eq!(base64_decode("SGVsbG8").unwrap(), "Hello");
    }

    #[test]
    fn test_base64_decode_json() {
        // Base64 of '{"type":"ping"}'
        let encoded = "eyJ0eXBlIjoicGluZyJ9";
        let decoded = base64_decode(encoded).unwrap();
        assert_eq!(decoded, r#"{"type":"ping"}"#);
    }

    #[test]
    fn test_urlencoding() {
        assert_eq!(urlencoding::encode("anthropic.claude-3"), "anthropic.claude-3");
        assert_eq!(urlencoding::encode("model/name"), "model%2Fname");
    }
}
