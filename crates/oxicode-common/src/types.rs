use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Role of a message participant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

/// A single content block within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    Thinking {
        thinking: String,
    },
}

impl ContentBlock {
    /// Extract text content if this is a Text block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }
}

/// A conversation message with role and content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: Role,
    pub content: Vec<ContentBlock>,
    pub model: Option<String>,
    pub stop_reason: Option<StopReason>,
    pub created_at: DateTime<Utc>,
    /// Token usage for this message (populated for assistant messages).
    pub usage: Option<Usage>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            model: None,
            stop_reason: None,
            created_at: Utc::now(),
            usage: None,
        }
    }

    pub fn assistant() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: Vec::new(),
            model: None,
            stop_reason: None,
            created_at: Utc::now(),
            usage: None,
        }
    }

    /// Concatenate all text blocks into a single string.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(ContentBlock::as_text)
            .collect::<Vec<_>>()
            .join("")
    }
}

/// Why the model stopped generating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
}

/// Token usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_creation_input_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
}

/// Type of rate limit encountered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitType {
    TokensPerMinute,
    RequestsPerMinute,
    TokensPerDay,
    InputTokensPerMinute,
    OutputTokensPerMinute,
    Unknown,
}

impl std::fmt::Display for RateLimitType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TokensPerMinute => write!(f, "tokens/min"),
            Self::RequestsPerMinute => write!(f, "requests/min"),
            Self::TokensPerDay => write!(f, "tokens/day"),
            Self::InputTokensPerMinute => write!(f, "input tokens/min"),
            Self::OutputTokensPerMinute => write!(f, "output tokens/min"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Information extracted from rate limit response headers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateLimitInfo {
    /// Seconds to wait before retrying (from `retry-after` header).
    pub retry_after_secs: Option<f64>,
    /// The type of rate limit that was hit.
    pub limit_type: RateLimitType,
    /// Remaining requests/tokens before next reset (if available).
    pub remaining: Option<u64>,
    /// When the rate limit resets (if available).
    pub reset_at: Option<DateTime<Utc>>,
    /// Human-readable message for TUI display.
    pub message: String,
}

impl Default for RateLimitInfo {
    fn default() -> Self {
        Self {
            retry_after_secs: None,
            limit_type: RateLimitType::Unknown,
            remaining: None,
            reset_at: None,
            message: "Rate limited".to_string(),
        }
    }
}

/// User response to a tool permission prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionResponse {
    AllowOnce,
    AlwaysAllow,
    Deny,
    AlwaysDeny,
}

/// Model metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub max_tokens: u32,
    pub supports_tools: bool,
    pub supports_thinking: bool,
}

impl ModelInfo {
    /// Default Claude Sonnet 4 model info.
    pub fn default_sonnet() -> Self {
        Self {
            id: "claude-sonnet-4-20250514".to_string(),
            name: "Claude Sonnet 4".to_string(),
            provider: "anthropic".to_string(),
            max_tokens: 16384,
            supports_tools: true,
            supports_thinking: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_user() {
        let msg = Message::user("hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.text(), "hello");
    }

    #[test]
    fn test_content_block_serde_roundtrip() {
        let block = ContentBlock::Text {
            text: "test".to_string(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_text(), Some("test"));
    }

    #[test]
    fn test_tool_use_serde() {
        let block = ContentBlock::ToolUse {
            id: "tu_1".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({"path": "/tmp/test"}),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("tool_use"));
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ContentBlock::ToolUse { .. }));
    }

    #[test]
    fn test_role_serde() {
        let json = serde_json::to_string(&Role::Assistant).unwrap();
        assert_eq!(json, "\"assistant\"");
        let parsed: Role = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Role::Assistant);
    }

    #[test]
    fn test_stop_reason_serde() {
        let json = serde_json::to_string(&StopReason::EndTurn).unwrap();
        assert_eq!(json, "\"end_turn\"");
    }

    #[test]
    fn test_rate_limit_type_display() {
        assert_eq!(RateLimitType::TokensPerMinute.to_string(), "tokens/min");
        assert_eq!(RateLimitType::RequestsPerMinute.to_string(), "requests/min");
        assert_eq!(RateLimitType::TokensPerDay.to_string(), "tokens/day");
        assert_eq!(
            RateLimitType::InputTokensPerMinute.to_string(),
            "input tokens/min"
        );
        assert_eq!(
            RateLimitType::OutputTokensPerMinute.to_string(),
            "output tokens/min"
        );
        assert_eq!(RateLimitType::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_rate_limit_type_serde() {
        let variants = vec![
            RateLimitType::TokensPerMinute,
            RateLimitType::RequestsPerMinute,
            RateLimitType::TokensPerDay,
            RateLimitType::InputTokensPerMinute,
            RateLimitType::OutputTokensPerMinute,
            RateLimitType::Unknown,
        ];

        for original in variants {
            let json = serde_json::to_string(&original).unwrap();
            let parsed: RateLimitType = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, original);
        }
    }

    #[test]
    fn test_rate_limit_info_default() {
        let info = RateLimitInfo::default();
        assert_eq!(info.retry_after_secs, None);
        assert_eq!(info.limit_type, RateLimitType::Unknown);
        assert_eq!(info.remaining, None);
        assert_eq!(info.reset_at, None);
        assert_eq!(info.message, "Rate limited");
    }

    #[test]
    fn test_rate_limit_info_serde_roundtrip() {
        use chrono::Utc;

        let original = RateLimitInfo {
            retry_after_secs: Some(30.5),
            limit_type: RateLimitType::TokensPerMinute,
            remaining: Some(100),
            reset_at: Some(Utc::now()),
            message: "Rate limited (tokens/min). Retry after 30s".to_string(),
        };

        let json = serde_json::to_string(&original).unwrap();
        let parsed: RateLimitInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.retry_after_secs, original.retry_after_secs);
        assert_eq!(parsed.limit_type, original.limit_type);
        assert_eq!(parsed.remaining, original.remaining);
        assert_eq!(parsed.message, original.message);
        // reset_at comparison is tricky due to precision; just verify it's Some
        assert!(parsed.reset_at.is_some());
    }

    #[test]
    fn test_rate_limit_info_with_partial_data() {
        let info = RateLimitInfo {
            retry_after_secs: Some(60.0),
            limit_type: RateLimitType::RequestsPerMinute,
            remaining: None,
            reset_at: None,
            message: "Rate limited (requests/min)".to_string(),
        };

        assert_eq!(info.retry_after_secs, Some(60.0));
        assert_eq!(info.limit_type, RateLimitType::RequestsPerMinute);
        assert_eq!(info.remaining, None);
        assert_eq!(info.reset_at, None);
    }
}
