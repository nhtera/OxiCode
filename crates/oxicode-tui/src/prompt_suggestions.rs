//! Prompt suggestions — context-aware follow-up prompt suggestions.
//!
//! Analyzes the last assistant message and tool calls to suggest relevant
//! follow-up actions the user might want to take.

use oxicode_common::{ContentBlock, Message, Role};

/// Maximum number of suggestions to display.
const MAX_SUGGESTIONS: usize = 3;

/// A suggested follow-up prompt.
#[derive(Debug, Clone)]
pub struct PromptSuggestion {
    /// Short display label (e.g., "Run tests").
    pub label: String,
    /// Full prompt text to send if selected.
    pub prompt: String,
}

/// Generate context-aware suggestions based on the last message(s).
pub fn suggest_prompts(messages: &[Message]) -> Vec<PromptSuggestion> {
    let mut suggestions = Vec::new();

    let last_assistant = messages
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant);

    let Some(msg) = last_assistant else {
        return default_suggestions();
    };

    let text = msg.text().to_lowercase();
    let has_tool_use = msg
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolUse { .. }));
    let has_error = msg.content.iter().any(|b| {
        matches!(b, ContentBlock::ToolResult { is_error, .. } if *is_error)
    });

    // Error context → suggest debugging.
    if has_error || text.contains("error") || text.contains("failed") {
        suggestions.push(PromptSuggestion {
            label: "Debug this".to_string(),
            prompt: "Can you debug and fix this error?".to_string(),
        });
        suggestions.push(PromptSuggestion {
            label: "Show logs".to_string(),
            prompt: "Show me the relevant error logs.".to_string(),
        });
    }

    // File edit context → suggest testing.
    if has_file_edit(&msg.content) {
        suggestions.push(PromptSuggestion {
            label: "Run tests".to_string(),
            prompt: "Run the tests to verify the changes.".to_string(),
        });
        suggestions.push(PromptSuggestion {
            label: "Review changes".to_string(),
            prompt: "Review the changes you just made for any issues.".to_string(),
        });
    }

    // Code generation context → suggest improvements.
    if text.contains("created") || text.contains("implemented") || text.contains("added") {
        suggestions.push(PromptSuggestion {
            label: "Add tests".to_string(),
            prompt: "Write tests for the code you just created.".to_string(),
        });
    }

    // Tool use context → suggest follow-up.
    if has_tool_use && suggestions.is_empty() {
        suggestions.push(PromptSuggestion {
            label: "Continue".to_string(),
            prompt: "Continue with the next step.".to_string(),
        });
    }

    // Conversation context → suggest commit if git-related.
    if text.contains("commit") || text.contains("git") || text.contains("push") {
        suggestions.push(PromptSuggestion {
            label: "Commit changes".to_string(),
            prompt: "Commit the current changes with a descriptive message.".to_string(),
        });
    }

    // Trim to max and add a default if empty.
    if suggestions.is_empty() {
        return default_suggestions();
    }

    suggestions.truncate(MAX_SUGGESTIONS);
    suggestions
}

/// Check if any content block is a file edit/write tool call.
fn has_file_edit(blocks: &[ContentBlock]) -> bool {
    blocks.iter().any(|b| {
        matches!(b, ContentBlock::ToolUse { name, .. }
            if name == "file_edit" || name == "file_write"
                || name == "Edit" || name == "Write")
    })
}

/// Default suggestions when no specific context is detected.
fn default_suggestions() -> Vec<PromptSuggestion> {
    vec![
        PromptSuggestion {
            label: "What can you do?".to_string(),
            prompt: "What can you help me with?".to_string(),
        },
        PromptSuggestion {
            label: "Explain codebase".to_string(),
            prompt: "Give me an overview of this codebase.".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_suggestions_when_empty() {
        let suggestions = suggest_prompts(&[]);
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].label, "What can you do?");
    }

    #[test]
    fn test_error_context_suggestions() {
        let msg = Message {
            id: "1".to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "Error: compilation failed".to_string(),
            }],
            model: None,
            stop_reason: None,
            created_at: chrono::Utc::now(),
            usage: None,
        };
        let suggestions = suggest_prompts(&[msg]);
        assert!(suggestions.iter().any(|s| s.label == "Debug this"));
    }

    #[test]
    fn test_file_edit_suggests_tests() {
        let msg = Message {
            id: "1".to_string(),
            role: Role::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "file_edit".to_string(),
                    input: serde_json::json!({"file_path": "/tmp/foo.rs"}),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: "ok".to_string(),
                    is_error: false,
                },
            ],
            model: None,
            stop_reason: None,
            created_at: chrono::Utc::now(),
            usage: None,
        };
        let suggestions = suggest_prompts(&[msg]);
        assert!(suggestions.iter().any(|s| s.label == "Run tests"));
    }

    #[test]
    fn test_max_suggestions_limit() {
        let msg = Message {
            id: "1".to_string(),
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "Error: failed to compile. Created fix. git commit needed.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "file_edit".to_string(),
                    input: serde_json::json!({}),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: "err".to_string(),
                    is_error: true,
                },
            ],
            model: None,
            stop_reason: None,
            created_at: chrono::Utc::now(),
            usage: None,
        };
        let suggestions = suggest_prompts(&[msg]);
        assert!(suggestions.len() <= MAX_SUGGESTIONS);
    }
}
