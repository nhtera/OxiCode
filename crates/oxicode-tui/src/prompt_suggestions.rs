//! Prompt suggestions — context-aware follow-up prompt suggestions.
//!
//! Analyzes the last assistant message, tool calls, and tool results to suggest
//! relevant follow-up actions the user might want to take.

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

/// Context signals extracted from the conversation for suggestion generation.
#[allow(clippy::struct_excessive_bools)]
struct MessageContext {
    has_bash_error: bool,
    has_test_failure: bool,
    has_test_pass: bool,
    has_file_edit: bool,
    has_file_read: bool,
    has_search: bool,
    has_any_tool: bool,
    has_any_error: bool,
    user_turn_count: usize,
}

/// Analyze the last assistant message and surrounding tool results to build context.
fn analyze_context(messages: &[Message]) -> MessageContext {
    let mut ctx = MessageContext {
        has_bash_error: false,
        has_test_failure: false,
        has_test_pass: false,
        has_file_edit: false,
        has_file_read: false,
        has_search: false,
        has_any_tool: false,
        has_any_error: false,
        user_turn_count: messages.iter().filter(|m| m.role == Role::User).count(),
    };

    // Scan last assistant message + all tool results that follow it.
    let last_assistant_idx = messages
        .iter()
        .rposition(|m| m.role == Role::Assistant);
    let scan_start = last_assistant_idx.unwrap_or(0);

    for msg in &messages[scan_start..] {
        for block in &msg.content {
            match block {
                ContentBlock::ToolUse { name, .. } => {
                    ctx.has_any_tool = true;
                    let n = name.to_lowercase();
                    if is_edit_tool(&n) {
                        ctx.has_file_edit = true;
                    }
                    if is_read_tool(&n) {
                        ctx.has_file_read = true;
                    }
                    if is_search_tool(&n) {
                        ctx.has_search = true;
                    }
                }
                ContentBlock::ToolResult {
                    content, is_error, ..
                } => {
                    if *is_error {
                        ctx.has_any_error = true;
                    }
                    analyze_tool_result(&mut ctx, content, *is_error);
                }
                _ => {}
            }
        }
    }

    ctx
}

/// Classify tool result content for bash/test signals.
fn analyze_tool_result(ctx: &mut MessageContext, content: &str, is_error: bool) {
    let lower = content.to_lowercase();

    // Bash error detection (tool marked as error + bash-like output).
    if is_error && (lower.contains("error") || lower.contains("failed") || lower.contains("panicked")) {
        ctx.has_bash_error = true;
    }

    // Test result detection — look for test runner output patterns.
    let looks_like_test = lower.contains("test result")
        || lower.contains("tests passed")
        || (lower.contains("test") && (lower.contains("passed") || lower.contains("failed")));

    if looks_like_test {
        // Distinguish actual failures from "0 failed" in pass summaries.
        // Rust test output: "test result: FAILED" or "X failed" where X > 0.
        let has_real_failure = (lower.contains("test result: failed")
            || lower.contains("test result: failure"))
            || (lower.contains("failed")
                && !lower.contains("0 failed"));
        if has_real_failure {
            ctx.has_test_failure = true;
        } else {
            ctx.has_test_pass = true;
        }
    }
}

fn is_edit_tool(name: &str) -> bool {
    matches!(
        name,
        "file_edit" | "file_write" | "edit" | "write" | "multiedit" | "notebookedit"
    )
}

fn is_read_tool(name: &str) -> bool {
    matches!(name, "file_read" | "read" | "cat")
}

fn is_search_tool(name: &str) -> bool {
    matches!(
        name,
        "grep" | "glob" | "web_search" | "file_search" | "search" | "websearch" | "webfetch"
    )
}

/// Generate context-aware suggestions based on the conversation.
pub fn suggest_prompts(messages: &[Message]) -> Vec<PromptSuggestion> {
    // No messages → first-time suggestions.
    let has_user_message = messages.iter().any(|m| m.role == Role::User);
    if !has_user_message {
        return first_time_suggestions();
    }

    let last_assistant = messages.iter().rev().find(|m| m.role == Role::Assistant);
    if last_assistant.is_none() {
        return first_time_suggestions();
    }

    let ctx = analyze_context(messages);
    let mut suggestions = Vec::new();

    // Priority-ordered: most specific contexts first.

    // 1. Test failure → highest priority (actionable).
    if ctx.has_test_failure {
        suggestions.push(PromptSuggestion {
            label: "Fix failing tests".into(),
            prompt: "Fix the failing tests and run them again.".into(),
        });
    }

    // 2. Bash/tool error.
    if ctx.has_bash_error {
        suggestions.push(PromptSuggestion {
            label: "Fix this error".into(),
            prompt: "Can you debug and fix this error?".into(),
        });
        suggestions.push(PromptSuggestion {
            label: "Show full output".into(),
            prompt: "Show me the full error output and explain what went wrong.".into(),
        });
    } else if ctx.has_any_error {
        suggestions.push(PromptSuggestion {
            label: "Debug this".into(),
            prompt: "Can you debug and fix this error?".into(),
        });
    }

    // 3. Test passed → suggest commit.
    if ctx.has_test_pass && !ctx.has_test_failure && suggestions.is_empty() {
        suggestions.push(PromptSuggestion {
            label: "Commit changes".into(),
            prompt: "Commit the current changes with a descriptive message.".into(),
        });
    }

    // 4. File edits → testing/review follow-up.
    if ctx.has_file_edit && suggestions.len() < MAX_SUGGESTIONS {
        if !suggestions.iter().any(|s| s.label.contains("test") || s.label.contains("Test")) {
            suggestions.push(PromptSuggestion {
                label: "Run tests".into(),
                prompt: "Run the tests to verify the changes.".into(),
            });
        }
        if suggestions.len() < MAX_SUGGESTIONS {
            suggestions.push(PromptSuggestion {
                label: "Review changes".into(),
                prompt: "Review the changes you just made for any issues.".into(),
            });
        }
    }

    // 5. Search results → explain/dig deeper.
    if ctx.has_search && suggestions.len() < MAX_SUGGESTIONS {
        suggestions.push(PromptSuggestion {
            label: "Explain results".into(),
            prompt: "Explain the search results you found.".into(),
        });
    }

    // 6. Code read → explain/improve.
    if ctx.has_file_read && !ctx.has_file_edit && suggestions.len() < MAX_SUGGESTIONS {
        suggestions.push(PromptSuggestion {
            label: "Explain this code".into(),
            prompt: "Explain this code and how it works.".into(),
        });
    }

    // 7. Generic tool use with no errors → continue.
    if ctx.has_any_tool && suggestions.is_empty() {
        suggestions.push(PromptSuggestion {
            label: "Continue".into(),
            prompt: "Continue with the next step.".into(),
        });
    }

    // 8. Multi-turn conversation fallback (no tool use).
    if suggestions.is_empty() && ctx.user_turn_count > 4 {
        suggestions.push(PromptSuggestion {
            label: "Summarize progress".into(),
            prompt: "Summarize what we've accomplished so far.".into(),
        });
        suggestions.push(PromptSuggestion {
            label: "What's next?".into(),
            prompt: "What should we work on next?".into(),
        });
    }

    // 9. Final fallback — conversational defaults.
    if suggestions.is_empty() {
        suggestions.push(PromptSuggestion {
            label: "Continue".into(),
            prompt: "Continue with the next step.".into(),
        });
        suggestions.push(PromptSuggestion {
            label: "Start new task".into(),
            prompt: "Let's start working on something new.".into(),
        });
    }

    suggestions.truncate(MAX_SUGGESTIONS);
    suggestions
}

/// Suggestions for the very first interaction (no user messages yet).
fn first_time_suggestions() -> Vec<PromptSuggestion> {
    vec![
        PromptSuggestion {
            label: "What can you do?".into(),
            prompt: "What can you help me with?".into(),
        },
        PromptSuggestion {
            label: "Explain codebase".into(),
            prompt: "Give me an overview of this codebase.".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_user_msg(text: &str) -> Message {
        Message {
            id: "u".into(),
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            model: None,
            stop_reason: None,
            created_at: chrono::Utc::now(),
            usage: None,
        }
    }

    fn make_assistant_with_tool(tool_name: &str, result: &str, is_error: bool) -> Vec<Message> {
        vec![
            Message {
                id: "a".into(),
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: tool_name.into(),
                    input: serde_json::json!({}),
                }],
                model: None,
                stop_reason: None,
                created_at: chrono::Utc::now(),
                usage: None,
            },
            Message {
                id: "r".into(),
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: result.into(),
                    is_error,
                }],
                model: None,
                stop_reason: None,
                created_at: chrono::Utc::now(),
                usage: None,
            },
        ]
    }

    #[test]
    fn test_first_time_suggestions() {
        let suggestions = suggest_prompts(&[]);
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].label, "What can you do?");
    }

    #[test]
    fn test_bash_error_suggests_fix() {
        let mut msgs = vec![make_user_msg("run cargo build")];
        msgs.extend(make_assistant_with_tool(
            "bash",
            "error[E0308]: mismatched types\nfailed to compile",
            true,
        ));
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions.iter().any(|s| s.label == "Fix this error"),
            "Should suggest fixing bash error, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_test_failure_suggests_fix() {
        let mut msgs = vec![make_user_msg("run tests")];
        msgs.extend(make_assistant_with_tool(
            "bash",
            "test result: FAILED. 3 passed; 1 failed",
            true,
        ));
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions.iter().any(|s| s.label == "Fix failing tests"),
            "Should suggest fixing tests, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_test_pass_suggests_commit() {
        let mut msgs = vec![make_user_msg("run tests")];
        msgs.extend(make_assistant_with_tool(
            "bash",
            "test result: ok. 10 passed; 0 failed",
            false,
        ));
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions.iter().any(|s| s.label == "Commit changes"),
            "Should suggest commit after tests pass, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_file_edit_suggests_tests() {
        let mut msgs = vec![make_user_msg("edit the file")];
        msgs.extend(make_assistant_with_tool("file_edit", "ok", false));
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions.iter().any(|s| s.label == "Run tests"),
            "Should suggest running tests after edit, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_search_suggests_explain() {
        let mut msgs = vec![make_user_msg("find the function")];
        msgs.extend(make_assistant_with_tool("grep", "found 3 matches", false));
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions.iter().any(|s| s.label == "Explain results"),
            "Should suggest explaining search results, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_file_read_suggests_explain_code() {
        let mut msgs = vec![make_user_msg("read main.rs")];
        msgs.extend(make_assistant_with_tool("Read", "fn main() {}", false));
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions.iter().any(|s| s.label == "Explain this code"),
            "Should suggest explaining code after read, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_generic_tool_suggests_continue() {
        let mut msgs = vec![make_user_msg("do something")];
        msgs.extend(make_assistant_with_tool("custom_tool", "done", false));
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions.iter().any(|s| s.label == "Continue"),
            "Should suggest continue for unknown tool, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_max_suggestions_limit() {
        let mut msgs = vec![make_user_msg("fix it")];
        // Bash error + file edit → many possible suggestions.
        msgs.push(Message {
            id: "a".into(),
            role: Role::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({}),
                },
                ContentBlock::ToolUse {
                    id: "t2".into(),
                    name: "file_edit".into(),
                    input: serde_json::json!({}),
                },
            ],
            model: None,
            stop_reason: None,
            created_at: chrono::Utc::now(),
            usage: None,
        });
        msgs.push(Message {
            id: "r".into(),
            role: Role::User,
            content: vec![
                ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "error: compilation failed".into(),
                    is_error: true,
                },
                ContentBlock::ToolResult {
                    tool_use_id: "t2".into(),
                    content: "ok".into(),
                    is_error: false,
                },
            ],
            model: None,
            stop_reason: None,
            created_at: chrono::Utc::now(),
            usage: None,
        });
        let suggestions = suggest_prompts(&msgs);
        assert!(suggestions.len() <= MAX_SUGGESTIONS);
    }

    #[test]
    fn test_multi_turn_fallback() {
        // >4 user messages with no tool use in last assistant message.
        let mut msgs: Vec<Message> = (0..5)
            .flat_map(|i| {
                vec![
                    make_user_msg(&format!("question {i}")),
                    Message {
                        id: format!("a{i}"),
                        role: Role::Assistant,
                        content: vec![ContentBlock::Text {
                            text: format!("answer {i}"),
                        }],
                        model: None,
                        stop_reason: None,
                        created_at: chrono::Utc::now(),
                        usage: None,
                    },
                ]
            })
            .collect();
        // Add one more user message to bring count > 4.
        msgs.push(make_user_msg("another question"));
        msgs.push(Message {
            id: "final".into(),
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "another answer".into(),
            }],
            model: None,
            stop_reason: None,
            created_at: chrono::Utc::now(),
            usage: None,
        });

        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions.iter().any(|s| s.label == "Summarize progress")
                || suggestions.iter().any(|s| s.label == "What's next?"),
            "Multi-turn should suggest summary/next, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }
}
