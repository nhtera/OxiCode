use oxicode_common::{StopReason, Usage};
use serde::{Deserialize, Serialize};

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
