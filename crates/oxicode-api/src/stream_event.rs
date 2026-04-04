use oxicode_common::{RateLimitInfo, StopReason, Usage};
use serde::{Deserialize, Serialize};

use crate::prompt_cache_detection::CacheBreakEvent;

/// Events emitted from an LLM streaming response.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// Stream error.
    Error { message: String },

    /// Ping / keep-alive (ignored by consumers).
    Ping,
}

/// Raw SSE event types from Anthropic API.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub(crate) enum RawSseEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: RawMessageStart },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: u32,
        content_block: RawContentBlock,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: u32, delta: RawDelta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u32 },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: RawMessageDeltaBody,
        usage: Option<RawUsage>,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "error")]
    Error { error: RawError },
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawMessageStart {
    pub usage: Option<RawUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub(crate) enum RawContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum RawDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawMessageDeltaBody {
    pub stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_creation_input_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawError {
    pub message: String,
}

/// Convert raw SSE event to our `StreamEvent`(s).
/// Returns a Vec because some SSE events (e.g., `message_delta`) produce
/// both a usage update and a stop event (H2 fix).
impl RawSseEvent {
    pub fn into_stream_events(self) -> Vec<StreamEvent> {
        match self {
            Self::MessageStart { message } => message.usage.map_or_else(Vec::new, |u| {
                vec![StreamEvent::UsageUpdate(Usage {
                    input_tokens: u.input_tokens.unwrap_or(0),
                    output_tokens: u.output_tokens.unwrap_or(0),
                    cache_creation_input_tokens: u.cache_creation_input_tokens,
                    cache_read_input_tokens: u.cache_read_input_tokens,
                })]
            }),
            Self::ContentBlockStart {
                content_block: RawContentBlock::ToolUse { id, name },
                ..
            } => vec![StreamEvent::ToolUseStart { id, name }],
            Self::ContentBlockStart { .. } => vec![],
            Self::ContentBlockDelta { delta, .. } => match delta {
                RawDelta::TextDelta { text } => vec![StreamEvent::TextDelta { text }],
                RawDelta::InputJsonDelta { partial_json } => {
                    vec![StreamEvent::ToolInputDelta { partial_json }]
                }
                RawDelta::ThinkingDelta { thinking } => {
                    vec![StreamEvent::ThinkingDelta { thinking }]
                }
            },
            Self::ContentBlockStop { index } => vec![StreamEvent::ContentBlockStop { index }],
            Self::MessageDelta { delta, usage } => {
                let mut events = Vec::new();
                let stop_reason = delta.stop_reason.and_then(|s| match s.as_str() {
                    "end_turn" => Some(StopReason::EndTurn),
                    "tool_use" => Some(StopReason::ToolUse),
                    "max_tokens" => Some(StopReason::MaxTokens),
                    "stop_sequence" => Some(StopReason::StopSequence),
                    _ => None,
                });
                // H2 FIX: Emit BOTH usage and stop reason when both are present.
                if let Some(u) = usage {
                    events.push(StreamEvent::UsageUpdate(Usage {
                        input_tokens: u.input_tokens.unwrap_or(0),
                        output_tokens: u.output_tokens.unwrap_or(0),
                        cache_creation_input_tokens: u.cache_creation_input_tokens,
                        cache_read_input_tokens: u.cache_read_input_tokens,
                    }));
                }
                if let Some(reason) = stop_reason {
                    events.push(StreamEvent::MessageStop {
                        stop_reason: reason,
                    });
                }
                events
            }
            Self::MessageStop => vec![StreamEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
            }],
            Self::Ping => vec![StreamEvent::Ping],
            Self::Error { error } => vec![StreamEvent::Error {
                message: error.message,
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thinking_delta_event() {
        let json = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me think..."}}"#;
        let raw: RawSseEvent = serde_json::from_str(json).unwrap();
        let events = raw.into_stream_events();
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], StreamEvent::ThinkingDelta { thinking } if thinking == "Let me think...")
        );
    }

    #[test]
    fn test_thinking_content_block_start() {
        let json = r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#;
        let raw: RawSseEvent = serde_json::from_str(json).unwrap();
        let events = raw.into_stream_events();
        // Thinking block start produces no events (same as text block start).
        assert!(events.is_empty());
    }

    #[test]
    fn test_cache_usage_in_message_start() {
        let json = r#"{"type":"message_start","message":{"usage":{"input_tokens":100,"output_tokens":0,"cache_creation_input_tokens":50,"cache_read_input_tokens":25}}}"#;
        let raw: RawSseEvent = serde_json::from_str(json).unwrap();
        let events = raw.into_stream_events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::UsageUpdate(usage) => {
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(usage.cache_creation_input_tokens, Some(50));
                assert_eq!(usage.cache_read_input_tokens, Some(25));
            }
            _ => panic!("Expected UsageUpdate"),
        }
    }

    #[test]
    fn test_cache_usage_in_message_delta() {
        let json = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42,"cache_creation_input_tokens":10,"cache_read_input_tokens":5}}"#;
        let raw: RawSseEvent = serde_json::from_str(json).unwrap();
        let events = raw.into_stream_events();
        // Should produce both UsageUpdate and MessageStop.
        assert_eq!(events.len(), 2);
        match &events[0] {
            StreamEvent::UsageUpdate(usage) => {
                assert_eq!(usage.output_tokens, 42);
                assert_eq!(usage.cache_creation_input_tokens, Some(10));
                assert_eq!(usage.cache_read_input_tokens, Some(5));
            }
            _ => panic!("Expected UsageUpdate"),
        }
        assert!(matches!(
            &events[1],
            StreamEvent::MessageStop {
                stop_reason: StopReason::EndTurn
            }
        ));
    }

    #[test]
    fn test_stream_event_rate_limited_serde() {
        use oxicode_common::RateLimitType;

        let info = RateLimitInfo {
            retry_after_secs: Some(30.0),
            limit_type: RateLimitType::TokensPerMinute,
            remaining: Some(0),
            message: "Rate limited (tokens/min). Retry after 30s".to_string(),
            ..Default::default()
        };

        let event = StreamEvent::RateLimited {
            info,
            attempt: 1,
            max_retries: 3,
            retry_in_secs: 30.0,
        };

        let json = serde_json::to_string(&event).unwrap();
        let parsed: StreamEvent = serde_json::from_str(&json).unwrap();

        match parsed {
            StreamEvent::RateLimited {
                info: parsed_info,
                attempt,
                max_retries,
                retry_in_secs,
            } => {
                assert_eq!(parsed_info.retry_after_secs, Some(30.0));
                assert_eq!(parsed_info.limit_type, RateLimitType::TokensPerMinute);
                assert_eq!(parsed_info.remaining, Some(0));
                assert_eq!(attempt, 1);
                assert_eq!(max_retries, 3);
                assert_eq!(retry_in_secs, 30.0);
            }
            _ => panic!("Expected RateLimited event"),
        }
    }

    #[test]
    fn test_stream_event_rate_limited_with_different_types() {
        use oxicode_common::RateLimitType;

        let test_cases = vec![
            (RateLimitType::TokensPerMinute, "tokens/min"),
            (RateLimitType::RequestsPerMinute, "requests/min"),
            (RateLimitType::TokensPerDay, "tokens/day"),
            (RateLimitType::InputTokensPerMinute, "input tokens/min"),
            (RateLimitType::OutputTokensPerMinute, "output tokens/min"),
        ];

        for (limit_type, _expected_str) in test_cases {
            let info = RateLimitInfo {
                limit_type,
                retry_after_secs: Some(60.0),
                ..Default::default()
            };

            let event = StreamEvent::RateLimited {
                info,
                attempt: 2,
                max_retries: 3,
                retry_in_secs: 60.0,
            };

            let json = serde_json::to_string(&event).unwrap();
            let parsed: StreamEvent = serde_json::from_str(&json).unwrap();

            match parsed {
                StreamEvent::RateLimited {
                    info: parsed_info, ..
                } => {
                    assert_eq!(parsed_info.limit_type, limit_type);
                }
                _ => panic!("Expected RateLimited event"),
            }
        }
    }

    #[test]
    fn test_stream_event_text_delta_vs_rate_limited() {
        use oxicode_common::RateLimitType;

        let text_event = StreamEvent::TextDelta {
            text: "Hello".to_string(),
        };
        let text_json = serde_json::to_string(&text_event).unwrap();
        assert!(text_json.contains("\"TextDelta\"") || text_json.contains("text_delta"));

        let rate_limited_event = StreamEvent::RateLimited {
            info: RateLimitInfo {
                limit_type: RateLimitType::TokensPerMinute,
                ..Default::default()
            },
            attempt: 1,
            max_retries: 3,
            retry_in_secs: 30.0,
        };
        let rate_limited_json = serde_json::to_string(&rate_limited_event).unwrap();
        assert!(
            rate_limited_json.contains("\"RateLimited\"")
                || rate_limited_json.contains("rate_limited")
        );

        // Ensure they serialize differently
        assert_ne!(text_json, rate_limited_json);
    }
}
