//! NDJSON structured output mode for programmatic consumption.
//!
//! When `--output json` is passed, all events are serialized as newline-delimited
//! JSON to stdout. TUI output redirects to stderr in this mode.
//! Enables: IDE extensions, CI/CD pipelines, automated testing, remote agents.

use std::io::{self, Write};

use chrono::Utc;
use serde::Serialize;

/// All event types emitted in NDJSON output mode.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NdjsonEvent {
    /// Emitted once at the start of a session.
    SessionStart {
        session_id: String,
        model: String,
        timestamp: String,
    },
    /// User message received.
    UserMessage { content: String },
    /// Streaming text delta from assistant.
    AssistantText { text: String },
    /// Tool invocation by the assistant.
    ToolUse {
        tool_name: String,
        input: serde_json::Value,
    },
    /// Tool execution result.
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    /// Token usage statistics for a turn.
    Usage {
        input_tokens: u32,
        output_tokens: u32,
    },
    /// Turn completed.
    TurnComplete { stop_reason: String },
    /// Error occurred.
    Error { message: String },
    /// Session ended.
    SessionEnd { reason: String },
}

/// Writer that serializes events as NDJSON to stdout.
pub struct NdjsonWriter {
    writer: io::Stdout,
}

impl NdjsonWriter {
    pub fn new() -> Self {
        Self {
            writer: io::stdout(),
        }
    }

    /// Write a single event as a JSON line to stdout.
    pub fn emit(&mut self, event: &NdjsonEvent) -> io::Result<()> {
        let json = serde_json::to_string(event).map_err(io::Error::other)?;
        let mut lock = self.writer.lock();
        lock.write_all(json.as_bytes())?;
        lock.write_all(b"\n")?;
        lock.flush()
    }

    /// Emit a session_start event.
    pub fn session_start(&mut self, session_id: &str, model: &str) -> io::Result<()> {
        self.emit(&NdjsonEvent::SessionStart {
            session_id: session_id.to_string(),
            model: model.to_string(),
            timestamp: Utc::now().to_rfc3339(),
        })
    }

    /// Emit a user_message event.
    pub fn user_message(&mut self, content: &str) -> io::Result<()> {
        self.emit(&NdjsonEvent::UserMessage {
            content: content.to_string(),
        })
    }

    /// Emit a text delta from the assistant.
    pub fn assistant_text(&mut self, text: &str) -> io::Result<()> {
        self.emit(&NdjsonEvent::AssistantText {
            text: text.to_string(),
        })
    }

    /// Emit a tool_use event.
    pub fn tool_use(&mut self, tool_name: &str, input: &serde_json::Value) -> io::Result<()> {
        self.emit(&NdjsonEvent::ToolUse {
            tool_name: tool_name.to_string(),
            input: input.clone(),
        })
    }

    /// Emit a tool_result event.
    pub fn tool_result(
        &mut self,
        tool_use_id: &str,
        content: &str,
        is_error: bool,
    ) -> io::Result<()> {
        self.emit(&NdjsonEvent::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            content: content.to_string(),
            is_error,
        })
    }

    /// Emit a turn_complete event.
    pub fn turn_complete(&mut self, stop_reason: &str) -> io::Result<()> {
        self.emit(&NdjsonEvent::TurnComplete {
            stop_reason: stop_reason.to_string(),
        })
    }

    /// Emit an error event.
    pub fn error(&mut self, message: &str) -> io::Result<()> {
        self.emit(&NdjsonEvent::Error {
            message: message.to_string(),
        })
    }

    /// Emit session_end event.
    pub fn session_end(&mut self, reason: &str) -> io::Result<()> {
        self.emit(&NdjsonEvent::SessionEnd {
            reason: reason.to_string(),
        })
    }
}

impl Default for NdjsonWriter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ndjson_event_serialization() {
        let event = NdjsonEvent::AssistantText {
            text: "Hello".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"assistant_text\""));
        assert!(json.contains("\"text\":\"Hello\""));
    }

    #[test]
    fn test_session_start_event() {
        let event = NdjsonEvent::SessionStart {
            session_id: "abc".to_string(),
            model: "claude-opus-4-6".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"session_start\""));
        assert!(json.contains("\"session_id\":\"abc\""));
    }

    #[test]
    fn test_tool_use_event() {
        let event = NdjsonEvent::ToolUse {
            tool_name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"tool_use\""));
        assert!(json.contains("\"tool_name\":\"bash\""));
    }

    #[test]
    fn test_error_event() {
        let event = NdjsonEvent::Error {
            message: "connection failed".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"error\""));
    }
}
