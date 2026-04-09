//! Prompt suggestions — context-aware follow-up prompt suggestions.
//!
//! Analyzes the last assistant message, tool calls, tool results, and
//! conversation text to suggest relevant follow-up actions.

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
    /// Assistant's last text asks a question or requests input.
    assistant_asks_question: bool,
    /// Assistant offered choices / options.
    assistant_offers_choices: bool,
    /// Assistant confirmed task completion.
    assistant_task_done: bool,
    /// Assistant's response text (lowercase, for pattern matching).
    assistant_text_lower: String,
    /// User's last message text (lowercase).
    user_text_lower: String,
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
        assistant_asks_question: false,
        assistant_offers_choices: false,
        assistant_task_done: false,
        assistant_text_lower: String::new(),
        user_text_lower: String::new(),
    };

    // Extract user's last text message.
    for msg in messages.iter().rev() {
        if msg.role == Role::User {
            for block in &msg.content {
                if let ContentBlock::Text { text } = block {
                    ctx.user_text_lower = text.to_lowercase();
                    break;
                }
            }
            if !ctx.user_text_lower.is_empty() {
                break;
            }
        }
    }

    // Scan last assistant message + all tool results that follow it.
    let last_assistant_idx = messages.iter().rposition(|m| m.role == Role::Assistant);
    let scan_start = last_assistant_idx.unwrap_or(0);

    for msg in &messages[scan_start..] {
        for block in &msg.content {
            match block {
                ContentBlock::Text { text } if msg.role == Role::Assistant => {
                    let lower = text.to_lowercase();
                    ctx.assistant_text_lower.push_str(&lower);
                    ctx.assistant_text_lower.push(' ');
                }
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

    // Analyze assistant text patterns.
    let at = &ctx.assistant_text_lower;
    ctx.assistant_asks_question = at.contains('?')
        || at.contains("what would you")
        || at.contains("what do you")
        || at.contains("would you like")
        || at.contains("how can i help")
        || at.contains("what can i")
        || at.contains("let me know")
        || at.contains("tell me more")
        || at.contains("could you")
        || at.contains("what should")
        || at.contains("shall i");

    ctx.assistant_offers_choices = at.contains("option")
        || at.contains("approach")
        || at.contains("choose")
        || at.contains("alternative")
        || at.contains("we could")
        || at.contains("you could")
        || at.contains("1.");

    ctx.assistant_task_done = at.contains("done")
        || at.contains("complete")
        || at.contains("finished")
        || at.contains("all set")
        || at.contains("ready to")
        || at.contains("successfully")
        || at.contains("implemented")
        || at.contains("fixed the");

    ctx
}

/// Classify tool result content for bash/test signals.
fn analyze_tool_result(ctx: &mut MessageContext, content: &str, is_error: bool) {
    let lower = content.to_lowercase();

    if is_error
        && (lower.contains("error") || lower.contains("failed") || lower.contains("panicked"))
    {
        ctx.has_bash_error = true;
    }

    let looks_like_test = lower.contains("test result")
        || lower.contains("tests passed")
        || (lower.contains("test") && (lower.contains("passed") || lower.contains("failed")));

    if looks_like_test {
        let has_real_failure = (lower.contains("test result: failed")
            || lower.contains("test result: failure"))
            || (lower.contains("failed") && !lower.contains("0 failed"));
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

    // --- Priority 1: Tool-based signals (most actionable) ---

    if ctx.has_test_failure {
        suggestions.push(PromptSuggestion {
            label: "Fix failing tests".into(),
            prompt: "Fix the failing tests and run them again.".into(),
        });
    }

    if ctx.has_bash_error {
        suggestions.push(PromptSuggestion {
            label: "Fix this error".into(),
            prompt: "Can you debug and fix this error?".into(),
        });
        if suggestions.len() < MAX_SUGGESTIONS {
            suggestions.push(PromptSuggestion {
                label: "Show full output".into(),
                prompt: "Show me the full error output and explain what went wrong.".into(),
            });
        }
    } else if ctx.has_any_error {
        suggestions.push(PromptSuggestion {
            label: "Debug this".into(),
            prompt: "Can you debug and fix this error?".into(),
        });
    }

    if ctx.has_test_pass && !ctx.has_test_failure && suggestions.is_empty() {
        suggestions.push(PromptSuggestion {
            label: "Commit changes".into(),
            prompt: "Commit the current changes with a descriptive message.".into(),
        });
    }

    if ctx.has_file_edit && suggestions.len() < MAX_SUGGESTIONS {
        if !suggestions
            .iter()
            .any(|s| s.label.contains("test") || s.label.contains("Test"))
        {
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

    if ctx.has_search && suggestions.len() < MAX_SUGGESTIONS {
        suggestions.push(PromptSuggestion {
            label: "Explain results".into(),
            prompt: "Explain the search results you found.".into(),
        });
    }

    if ctx.has_file_read && !ctx.has_file_edit && suggestions.len() < MAX_SUGGESTIONS {
        suggestions.push(PromptSuggestion {
            label: "Explain this code".into(),
            prompt: "Explain this code and how it works.".into(),
        });
    }

    if ctx.has_any_tool && suggestions.is_empty() {
        suggestions.push(PromptSuggestion {
            label: "Continue".into(),
            prompt: "Continue with the next step.".into(),
        });
    }

    // --- Priority 2: Text-based signals (assistant asked/offered/completed) ---

    if suggestions.is_empty() {
        // Assistant completed a task → suggest follow-ups.
        if ctx.assistant_task_done {
            add_task_done_suggestions(&ctx, &mut suggestions);
        }
        // Assistant offered choices/approaches (check before generic question).
        else if ctx.assistant_offers_choices {
            suggestions.push(PromptSuggestion {
                label: "Go with first".into(),
                prompt: "Go with the first approach.".into(),
            });
            suggestions.push(PromptSuggestion {
                label: "Compare options".into(),
                prompt: "Compare the options and recommend the best one.".into(),
            });
        }
        // Assistant asked a question or awaits input.
        else if ctx.assistant_asks_question {
            add_question_response_suggestions(&ctx, &mut suggestions);
        }
    }

    // --- Priority 3: User intent signals ---

    if suggestions.is_empty() {
        add_user_intent_suggestions(&ctx, &mut suggestions);
    }

    // --- Priority 4: Multi-turn fallback ---

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

    // --- Priority 5: Conversation-aware fallback ---

    if suggestions.is_empty() {
        add_conversation_fallback_suggestions(&ctx, &mut suggestions);
    }

    suggestions.truncate(MAX_SUGGESTIONS);
    suggestions
}

/// Suggestions after assistant says task is done.
fn add_task_done_suggestions(ctx: &MessageContext, suggestions: &mut Vec<PromptSuggestion>) {
    let at = &ctx.assistant_text_lower;

    // Code-related completion.
    if at.contains("implement")
        || at.contains("code")
        || at.contains("function")
        || at.contains("fix")
    {
        suggestions.push(PromptSuggestion {
            label: "Run tests".into(),
            prompt: "Run the tests to verify everything works.".into(),
        });
        if suggestions.len() < MAX_SUGGESTIONS {
            suggestions.push(PromptSuggestion {
                label: "Commit changes".into(),
                prompt: "Commit the current changes with a descriptive message.".into(),
            });
        }
    } else {
        suggestions.push(PromptSuggestion {
            label: "What's next?".into(),
            prompt: "What should we work on next?".into(),
        });
    }

    if suggestions.len() < MAX_SUGGESTIONS {
        suggestions.push(PromptSuggestion {
            label: "Review changes".into(),
            prompt: "Show me a summary of all changes made.".into(),
        });
    }
}

/// Suggestions when assistant asked a question.
fn add_question_response_suggestions(
    ctx: &MessageContext,
    suggestions: &mut Vec<PromptSuggestion>,
) {
    let at = &ctx.assistant_text_lower;
    let ut = &ctx.user_text_lower;

    // "What would you like to work on?" / "What do you have in mind?"
    if at.contains("work on") || at.contains("have in mind") || at.contains("get started") {
        // Detect project type from user text or suggest common workflows.
        if ut.contains("rust") || ut.contains("cargo") {
            suggestions.push(PromptSuggestion {
                label: "Run cargo check".into(),
                prompt: "Run cargo check to see the current state.".into(),
            });
            suggestions.push(PromptSuggestion {
                label: "Explore codebase".into(),
                prompt: "Give me an overview of the codebase structure.".into(),
            });
        } else {
            suggestions.push(PromptSuggestion {
                label: "Explore codebase".into(),
                prompt: "Give me an overview of the codebase structure.".into(),
            });
            suggestions.push(PromptSuggestion {
                label: "Find bugs".into(),
                prompt: "Look for potential bugs or issues in the code.".into(),
            });
        }
        if suggestions.len() < MAX_SUGGESTIONS {
            suggestions.push(PromptSuggestion {
                label: "Run tests".into(),
                prompt: "Run the test suite and report any failures.".into(),
            });
        }
        return;
    }

    // "Shall I continue?" / "Want me to proceed?"
    if at.contains("shall i")
        || at.contains("want me to")
        || at.contains("proceed")
        || at.contains("go ahead")
    {
        suggestions.push(PromptSuggestion {
            label: "Yes, go ahead".into(),
            prompt: "Yes, go ahead.".into(),
        });
        suggestions.push(PromptSuggestion {
            label: "Explain first".into(),
            prompt: "Explain your approach before proceeding.".into(),
        });
        return;
    }

    // "How can I help?" / generic question.
    if at.contains("how can i help") || at.contains("what can i") {
        suggestions.push(PromptSuggestion {
            label: "Explore codebase".into(),
            prompt: "Give me an overview of the codebase structure.".into(),
        });
        suggestions.push(PromptSuggestion {
            label: "Fix a bug".into(),
            prompt: "Help me find and fix bugs in the code.".into(),
        });
        if suggestions.len() < MAX_SUGGESTIONS {
            suggestions.push(PromptSuggestion {
                label: "Add a feature".into(),
                prompt: "I want to add a new feature. Let me describe it.".into(),
            });
        }
        return;
    }

    // Generic question fallback.
    suggestions.push(PromptSuggestion {
        label: "Yes".into(),
        prompt: "Yes.".into(),
    });
    suggestions.push(PromptSuggestion {
        label: "Explain more".into(),
        prompt: "Can you explain in more detail?".into(),
    });
}

/// Suggestions based on what the user was talking about.
fn add_user_intent_suggestions(ctx: &MessageContext, suggestions: &mut Vec<PromptSuggestion>) {
    let ut = &ctx.user_text_lower;

    // User talked about building/implementing.
    if ut.contains("build")
        || ut.contains("implement")
        || ut.contains("create")
        || ut.contains("add")
    {
        suggestions.push(PromptSuggestion {
            label: "Start implementing".into(),
            prompt: "Start implementing it. Show me the plan first.".into(),
        });
        suggestions.push(PromptSuggestion {
            label: "Explore first".into(),
            prompt: "First, explore the relevant code to understand the current state.".into(),
        });
        return;
    }

    // User talked about fixing/debugging.
    if ut.contains("fix") || ut.contains("bug") || ut.contains("debug") || ut.contains("error") {
        suggestions.push(PromptSuggestion {
            label: "Show the error".into(),
            prompt: "Let me show you the error output.".into(),
        });
        suggestions.push(PromptSuggestion {
            label: "Find root cause".into(),
            prompt: "Investigate and find the root cause.".into(),
        });
        return;
    }

    // User talked about testing.
    if ut.contains("test") {
        suggestions.push(PromptSuggestion {
            label: "Run all tests".into(),
            prompt: "Run the full test suite.".into(),
        });
        suggestions.push(PromptSuggestion {
            label: "Fix failures".into(),
            prompt: "Fix any failing tests.".into(),
        });
        return;
    }

    // User talked about reviewing/refactoring.
    if ut.contains("review")
        || ut.contains("refactor")
        || ut.contains("improve")
        || ut.contains("clean")
    {
        suggestions.push(PromptSuggestion {
            label: "Show suggestions".into(),
            prompt: "Show me specific improvement suggestions.".into(),
        });
        suggestions.push(PromptSuggestion {
            label: "Apply changes".into(),
            prompt: "Go ahead and apply the improvements.".into(),
        });
        return;
    }
}

/// Fallback suggestions with some conversation awareness.
fn add_conversation_fallback_suggestions(
    ctx: &MessageContext,
    suggestions: &mut Vec<PromptSuggestion>,
) {
    let at = &ctx.assistant_text_lower;

    // If assistant mentioned code/files, suggest exploring.
    if at.contains(".rs")
        || at.contains(".ts")
        || at.contains(".py")
        || at.contains("function")
        || at.contains("struct")
    {
        suggestions.push(PromptSuggestion {
            label: "Explore codebase".into(),
            prompt: "Give me an overview of the codebase structure and key files.".into(),
        });
        suggestions.push(PromptSuggestion {
            label: "Run tests".into(),
            prompt: "Run the tests to check the current state.".into(),
        });
        if suggestions.len() < MAX_SUGGESTIONS {
            suggestions.push(PromptSuggestion {
                label: "Find issues".into(),
                prompt: "Look for potential bugs or code quality issues.".into(),
            });
        }
        return;
    }

    // If assistant mentioned building/architecture.
    if at.contains("crate")
        || at.contains("workspace")
        || at.contains("module")
        || at.contains("architecture")
    {
        suggestions.push(PromptSuggestion {
            label: "Show structure".into(),
            prompt: "Show me the project structure and dependencies.".into(),
        });
        suggestions.push(PromptSuggestion {
            label: "Run cargo check".into(),
            prompt: "Run cargo check to verify the build.".into(),
        });
        return;
    }

    // True generic fallback — still useful.
    suggestions.push(PromptSuggestion {
        label: "Explore codebase".into(),
        prompt: "Give me an overview of the codebase.".into(),
    });
    suggestions.push(PromptSuggestion {
        label: "Run tests".into(),
        prompt: "Run the test suite and report results.".into(),
    });
    suggestions.push(PromptSuggestion {
        label: "Find issues".into(),
        prompt: "Look for bugs or issues in the code.".into(),
    });
}

/// Suggestions for the very first interaction (no user messages yet).
fn first_time_suggestions() -> Vec<PromptSuggestion> {
    vec![
        PromptSuggestion {
            label: "Explore codebase".into(),
            prompt: "Give me an overview of this codebase.".into(),
        },
        PromptSuggestion {
            label: "Run tests".into(),
            prompt: "Run the test suite and report any issues.".into(),
        },
        PromptSuggestion {
            label: "Find bugs".into(),
            prompt: "Look for potential bugs or issues in the code.".into(),
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

    fn make_assistant_msg(text: &str) -> Message {
        Message {
            id: "a".into(),
            role: Role::Assistant,
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
        assert_eq!(suggestions.len(), 3);
        assert_eq!(suggestions[0].label, "Explore codebase");
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
        let mut msgs: Vec<Message> = (0..5)
            .flat_map(|i| {
                vec![
                    make_user_msg(&format!("question {i}")),
                    make_assistant_msg(&format!("answer {i}")),
                ]
            })
            .collect();
        msgs.push(make_user_msg("another question"));
        msgs.push(make_assistant_msg("another answer"));

        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions.iter().any(|s| s.label == "Summarize progress")
                || suggestions.iter().any(|s| s.label == "What's next?"),
            "Multi-turn should suggest summary/next, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    // --- New text-analysis tests ---

    #[test]
    fn test_assistant_question_suggests_contextual_response() {
        let msgs = vec![
            make_user_msg("Let's start working on something new."),
            make_assistant_msg("Sure! What do you have in mind?"),
        ];
        let suggestions = suggest_prompts(&msgs);
        // Should NOT be generic "Continue" / "Start new task".
        assert!(
            !suggestions.iter().any(|s| s.label == "Continue"),
            "Should not show generic 'Continue' when assistant asks question, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
        assert!(
            suggestions.iter().any(|s| s.label == "Explore codebase"
                || s.label == "Find bugs"
                || s.label == "Run tests"
                || s.label == "Run cargo check"),
            "Should show actionable suggestions, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_assistant_task_done_suggests_followup() {
        let msgs = vec![
            make_user_msg("Fix the bug"),
            make_assistant_msg("I've successfully fixed the bug in the authentication module."),
        ];
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions
                .iter()
                .any(|s| s.label == "Run tests" || s.label == "Commit changes"),
            "Should suggest tests or commit after task done, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_assistant_offers_choices() {
        let msgs = vec![
            make_user_msg("How should we implement caching?"),
            make_assistant_msg(
                "We could use: 1. Redis 2. In-memory cache. Which approach would you prefer?",
            ),
        ];
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions
                .iter()
                .any(|s| s.label == "Go with first" || s.label == "Compare options"),
            "Should suggest choosing option, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_assistant_shall_i_continue() {
        let msgs = vec![
            make_user_msg("Refactor the module"),
            make_assistant_msg("I've planned the refactoring. Shall I proceed with the changes?"),
        ];
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions.iter().any(|s| s.label == "Yes, go ahead"),
            "Should suggest 'Yes, go ahead', got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_user_intent_build() {
        let msgs = vec![
            make_user_msg("I want to build a new authentication system"),
            make_assistant_msg(
                "That sounds like a great project. Here's what we need to consider...",
            ),
        ];
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions
                .iter()
                .any(|s| s.label == "Start implementing" || s.label == "Explore first"),
            "Should suggest implementation actions, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_conversation_with_code_mentions() {
        let msgs = vec![
            make_user_msg("Tell me about this project"),
            make_assistant_msg("This is a Rust workspace with structs and functions in main.rs..."),
        ];
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions
                .iter()
                .any(|s| s.label == "Explore codebase" || s.label == "Run tests"),
            "Should suggest code-related actions, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_how_can_i_help_suggestions() {
        let msgs = vec![
            make_user_msg("hi"),
            make_assistant_msg("Hello! How can I help you today?"),
        ];
        let suggestions = suggest_prompts(&msgs);
        assert!(
            suggestions.iter().any(|s| s.label == "Explore codebase"),
            "Should suggest exploring when assistant asks how to help, got: {:?}",
            suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
        );
    }
}
