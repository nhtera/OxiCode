//! Integration tests for streaming markdown collector edge cases and
//! context-aware prompt suggestion engine.
//!
//! Covers: code fences across deltas, unicode, large payloads, nested
//! markdown, finalize edge cases, prompt suggestion context analysis,
//! tool signals, text signals, first-time/multi-turn fallback.
//!
//! No API key needed — pure logic tests.
//! Run with: `cargo test -p oxicode-tui --test tui_streaming_and_suggestions_integration_tests`

use oxicode_common::{ContentBlock, Message, Role};
use oxicode_tui::streaming_markdown::MarkdownStreamCollector;
use oxicode_tui::{suggest_prompts, PromptSuggestion, TipsService};

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

fn make_user(text: &str) -> Message {
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

fn make_assistant(text: &str) -> Message {
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

fn make_tool_result(tool_name: &str, result: &str, is_error: bool) -> Vec<Message> {
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

fn has_label(suggestions: &[PromptSuggestion], label: &str) -> bool {
    suggestions.iter().any(|s| s.label == label)
}

// ═══════════════════════════════════════════════════════════════════
// A. Streaming Markdown — Code Fence Edge Cases
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_code_fence_split_across_three_deltas() {
    let mut c = MarkdownStreamCollector::new();

    // Open fence.
    c.push_delta("```python\n");
    let l1 = c.commit_complete_lines();

    // Code lines.
    c.push_delta("def hello():\n    print('hi')\n");
    let l2 = c.commit_complete_lines();

    // Close fence.
    c.push_delta("```\n");
    let l3 = c.commit_complete_lines();

    let total = l1.len() + l2.len() + l3.len();
    assert!(
        total >= 3,
        "code block should produce ≥3 lines, got: {total}"
    );
}

#[test]
fn test_nested_code_fence_with_triple_backticks_in_content() {
    let mut c = MarkdownStreamCollector::new();

    // Content that contains triple backticks inside a code block is tricky.
    c.push_delta("````markdown\n");
    c.push_delta("```rust\n");
    c.push_delta("fn main() {}\n");
    c.push_delta("```\n");
    c.push_delta("````\n");

    let lines = c.commit_complete_lines();
    assert!(
        !lines.is_empty(),
        "nested code fences should produce output"
    );
}

#[test]
fn test_code_fence_with_language_tag_variations() {
    for lang in &["rust", "python", "javascript", "go", "c++", ""] {
        let mut c = MarkdownStreamCollector::new();
        c.push_delta(&format!("```{lang}\ncode here\n```\n"));
        let lines = c.commit_complete_lines();
        assert!(
            !lines.is_empty(),
            "code fence with lang '{lang}' should produce output"
        );
    }
}

#[test]
fn test_inline_code_not_confused_with_fence() {
    let mut c = MarkdownStreamCollector::new();
    c.push_delta("Use `cargo test` to run tests.\n");
    let lines = c.commit_complete_lines();
    assert!(!lines.is_empty());

    let raw: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect();
    assert!(raw.contains("cargo test"), "inline code content preserved");
}

// ═══════════════════════════════════════════════════════════════════
// B. Streaming Markdown — Unicode and Special Characters
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_unicode_emoji_in_stream() {
    let mut c = MarkdownStreamCollector::new();
    c.push_delta("Hello 🎉🚀 World!\n");
    let lines = c.commit_complete_lines();
    assert!(!lines.is_empty());
    let raw: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect();
    assert!(raw.contains("🎉"), "emoji should be preserved");
    assert!(raw.contains("🚀"), "emoji should be preserved");
}

#[test]
fn test_cjk_characters_in_stream() {
    let mut c = MarkdownStreamCollector::new();
    c.push_delta("日本語テスト\n中文测试\n한국어 테스트\n");
    let lines = c.commit_complete_lines();
    assert!(
        lines.len() >= 3,
        "CJK lines should produce ≥3 lines, got: {}",
        lines.len()
    );
}

#[test]
fn test_mixed_unicode_and_markdown() {
    let mut c = MarkdownStreamCollector::new();
    c.push_delta("**太字** と *斜体* テスト\n");
    let lines = c.commit_complete_lines();
    assert!(!lines.is_empty());
    let raw: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect();
    assert!(!raw.contains("**"), "bold markers should be parsed away");
    assert!(raw.contains("太字"), "CJK content preserved");
}

// ═══════════════════════════════════════════════════════════════════
// C. Streaming Markdown — Large Payload Performance
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_large_streaming_payload_1000_lines() {
    let mut c = MarkdownStreamCollector::new();

    // Simulate 1000 lines arriving in 100-line batches.
    for batch in 0..10 {
        let mut chunk = String::new();
        for line in 0..100 {
            chunk.push_str(&format!("Line {}: batch {batch}\n", batch * 100 + line));
        }
        c.push_delta(&chunk);
        let new_lines = c.commit_complete_lines();
        assert!(!new_lines.is_empty(), "batch {batch} should produce lines");
    }

    assert_eq!(
        c.lines().len(),
        1000,
        "should have exactly 1000 committed lines"
    );
}

#[test]
fn test_incremental_does_not_reparse_committed() {
    let mut c = MarkdownStreamCollector::new();

    // Commit 500 lines.
    for i in 0..500 {
        c.push_delta(&format!("Line {i}\n"));
    }
    let first = c.commit_complete_lines();
    let first_count = first.len();

    // Add 1 more line.
    c.push_delta("Last line\n");
    let second = c.commit_complete_lines();
    let second_count = second.len();

    // Total lines = sum of both commits (no reparsing).
    assert_eq!(c.lines().len(), first_count + second_count);
}

#[test]
fn test_many_small_deltas_character_by_character() {
    let mut c = MarkdownStreamCollector::new();

    // Push character by character (worst-case streaming).
    let text = "Hello World!\n";
    for ch in text.chars() {
        c.push_delta(&ch.to_string());
    }

    let lines = c.commit_complete_lines();
    assert!(
        !lines.is_empty(),
        "char-by-char should commit after newline"
    );
}

// ═══════════════════════════════════════════════════════════════════
// D. Streaming Markdown — Finalize Edge Cases
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_finalize_on_empty_buffer() {
    let mut c = MarkdownStreamCollector::new();
    let lines = c.finalize();
    assert!(lines.is_empty(), "finalize on empty buffer = no lines");
}

#[test]
fn test_finalize_after_full_commit() {
    let mut c = MarkdownStreamCollector::new();
    c.push_delta("complete\n");
    c.commit_complete_lines();

    // Nothing remaining.
    let final_lines = c.finalize();
    assert!(
        final_lines.is_empty(),
        "finalize after full commit = no lines"
    );
}

#[test]
fn test_finalize_with_only_trailing_fragment() {
    let mut c = MarkdownStreamCollector::new();
    c.push_delta("no newline at end");
    assert_eq!(c.trailing_fragment(), Some("no newline at end"));

    let lines = c.finalize();
    assert!(!lines.is_empty(), "finalize should emit trailing fragment");
    // After finalize, committed_byte_offset == buffer.len(), so trailing_fragment
    // depends on rfind('\n'). Since there's no newline, the full buffer is "trailing"
    // but it's already been committed. Just verify lines were produced.
}

#[test]
fn test_finalize_mixed_committed_and_trailing() {
    let mut c = MarkdownStreamCollector::new();
    c.push_delta("line one\nline two\npartial");
    let committed = c.commit_complete_lines();
    assert!(!committed.is_empty());
    assert_eq!(c.trailing_fragment(), Some("partial"));

    let final_lines = c.finalize();
    assert!(!final_lines.is_empty(), "finalize should emit 'partial'");
}

// ═══════════════════════════════════════════════════════════════════
// E. Streaming Markdown — Clear and Reuse
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_clear_and_reuse() {
    let mut c = MarkdownStreamCollector::new();

    // First session.
    c.push_delta("Session 1\n");
    c.commit_complete_lines();
    assert!(!c.lines().is_empty());

    // Clear for new session.
    c.clear();
    assert!(c.lines().is_empty());
    assert!(c.trailing_fragment().is_none());

    // Second session.
    c.push_delta("Session 2\n");
    let lines = c.commit_complete_lines();
    assert!(!lines.is_empty());
    assert_eq!(
        c.lines().len(),
        lines.len(),
        "should only have session 2 lines"
    );
}

#[test]
fn test_multiple_clear_cycles() {
    let mut c = MarkdownStreamCollector::new();

    for i in 0..5 {
        c.push_delta(&format!("Cycle {i}\n"));
        c.commit_complete_lines();
        assert!(!c.lines().is_empty());
        c.clear();
        assert!(c.lines().is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════
// F. Streaming Markdown — Markdown Formatting
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_heading_rendering() {
    let mut c = MarkdownStreamCollector::new();
    c.push_delta("# Heading 1\n## Heading 2\n### Heading 3\n");
    let lines = c.commit_complete_lines();
    assert!(lines.len() >= 3, "headings should produce lines");
}

#[test]
fn test_list_items_rendering() {
    let mut c = MarkdownStreamCollector::new();
    c.push_delta("- Item 1\n- Item 2\n- Item 3\n");
    let lines = c.commit_complete_lines();
    assert!(lines.len() >= 3, "list items should produce ≥3 lines");
}

#[test]
fn test_bold_italic_combined() {
    let mut c = MarkdownStreamCollector::new();
    c.push_delta("***bold italic*** and **bold** and *italic*\n");
    let lines = c.commit_complete_lines();
    let raw: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect();
    assert!(!raw.contains("***"), "bold italic markers should be parsed");
    assert!(raw.contains("bold italic"));
}

#[test]
fn test_link_rendering() {
    let mut c = MarkdownStreamCollector::new();
    c.push_delta("[Click here](https://example.com)\n");
    let lines = c.commit_complete_lines();
    assert!(!lines.is_empty());
    let raw: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect();
    assert!(raw.contains("Click here"), "link text should be rendered");
}

// ═══════════════════════════════════════════════════════════════════
// G. Prompt Suggestions — First-Time (No Messages)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_first_time_suggestions_content() {
    let suggestions = suggest_prompts(&[]);
    assert_eq!(suggestions.len(), 3);
    assert!(has_label(&suggestions, "Explore codebase"));
    assert!(has_label(&suggestions, "Run tests"));
    assert!(has_label(&suggestions, "Find bugs"));
}

#[test]
fn test_no_assistant_message_fallback() {
    // Only user message, no assistant response yet.
    let msgs = vec![make_user("hello")];
    let suggestions = suggest_prompts(&msgs);
    // Should fall back to first-time suggestions.
    assert_eq!(suggestions.len(), 3);
    assert!(has_label(&suggestions, "Explore codebase"));
}

// ═══════════════════════════════════════════════════════════════════
// H. Prompt Suggestions — Tool Signal Detection
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_bash_error_suggests_fix() {
    let mut msgs = vec![make_user("build it")];
    msgs.extend(make_tool_result("bash", "error: compilation failed", true));
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Fix this error"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_test_failure_suggests_fix_tests() {
    let mut msgs = vec![make_user("test it")];
    msgs.extend(make_tool_result(
        "bash",
        "test result: FAILED. 5 passed; 2 failed",
        true,
    ));
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Fix failing tests"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_all_tests_pass_suggests_commit() {
    let mut msgs = vec![make_user("run tests")];
    msgs.extend(make_tool_result(
        "bash",
        "test result: ok. 20 passed; 0 failed",
        false,
    ));
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Commit changes"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_file_edit_suggests_run_tests() {
    let mut msgs = vec![make_user("fix the bug")];
    msgs.extend(make_tool_result("file_edit", "ok", false));
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Run tests"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_grep_suggests_explain_results() {
    let mut msgs = vec![make_user("find it")];
    msgs.extend(make_tool_result("grep", "found 5 matches", false));
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Explain results"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_file_read_only_suggests_explain_code() {
    let mut msgs = vec![make_user("show me the code")];
    msgs.extend(make_tool_result(
        "file_read",
        "fn main() { println!(\"hello\"); }",
        false,
    ));
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Explain this code"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════
// I. Prompt Suggestions — Assistant Text Signals
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_assistant_asks_how_can_help() {
    let msgs = vec![
        make_user("hi"),
        make_assistant("Hello! How can I help you today?"),
    ];
    let suggestions = suggest_prompts(&msgs);
    assert!(has_label(&suggestions, "Explore codebase"));
    assert!(has_label(&suggestions, "Fix a bug"));
}

#[test]
fn test_assistant_shall_i_proceed() {
    let msgs = vec![
        make_user("refactor this"),
        make_assistant("I've planned the changes. Shall I proceed?"),
    ];
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Yes, go ahead"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_assistant_offers_choices() {
    let msgs = vec![
        make_user("how to cache?"),
        make_assistant("We could use: 1. Redis 2. Memcached. Which approach?"),
    ];
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Go with first") || has_label(&suggestions, "Compare options"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_assistant_task_done_code() {
    let msgs = vec![
        make_user("fix the auth bug"),
        make_assistant("I've successfully fixed the authentication code."),
    ];
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Run tests") || has_label(&suggestions, "Commit changes"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_assistant_task_done_generic() {
    let msgs = vec![
        make_user("document this"),
        make_assistant("All set! The documentation is complete."),
    ];
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "What's next?") || has_label(&suggestions, "Review changes"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════
// J. Prompt Suggestions — User Intent Detection
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_user_intent_build() {
    let msgs = vec![
        make_user("I want to build a new feature"),
        // Avoid question mark and question-trigger phrases to ensure user intent path is taken.
        make_assistant("That sounds like a solid plan. I can help with that."),
    ];
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Start implementing") || has_label(&suggestions, "Explore first"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_user_intent_debug() {
    let msgs = vec![
        make_user("I need to fix a bug in the login"),
        make_assistant("I'd be happy to help debug that."),
    ];
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Show the error") || has_label(&suggestions, "Find root cause"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_user_intent_test() {
    let msgs = vec![
        make_user("I need to test the API"),
        make_assistant("Let me help with testing."),
    ];
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Run all tests") || has_label(&suggestions, "Fix failures"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_user_intent_review() {
    let msgs = vec![
        make_user("Can you review and improve this module?"),
        make_assistant("Sure, let me take a look."),
    ];
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Show suggestions") || has_label(&suggestions, "Apply changes"),
        "got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════
// K. Prompt Suggestions — Multi-Turn Fallback
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_multi_turn_suggests_summary() {
    let mut msgs: Vec<Message> = (0..6)
        .flat_map(|i| {
            vec![
                make_user(&format!("q{i}")),
                make_assistant(&format!("a{i}")),
            ]
        })
        .collect();
    msgs.push(make_user("more"));
    msgs.push(make_assistant("more answer"));

    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Summarize progress") || has_label(&suggestions, "What's next?"),
        "multi-turn fallback should trigger, got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_max_suggestions_never_exceeds_three() {
    // Create a scenario with many signals.
    let mut msgs = vec![make_user("fix and test")];
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
            ContentBlock::ToolUse {
                id: "t3".into(),
                name: "grep".into(),
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
                content: "error: failed".into(),
                is_error: true,
            },
            ContentBlock::ToolResult {
                tool_use_id: "t2".into(),
                content: "ok".into(),
                is_error: false,
            },
            ContentBlock::ToolResult {
                tool_use_id: "t3".into(),
                content: "3 matches".into(),
                is_error: false,
            },
        ],
        model: None,
        stop_reason: None,
        created_at: chrono::Utc::now(),
        usage: None,
    });

    let suggestions = suggest_prompts(&msgs);
    assert!(
        suggestions.len() <= 3,
        "should never exceed 3, got: {}",
        suggestions.len()
    );
}

// ═══════════════════════════════════════════════════════════════════
// L. Prompt Suggestions — Conversation Fallback
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_fallback_code_mentions() {
    let msgs = vec![
        make_user("tell me about this"),
        make_assistant("The main.rs file contains a struct for managing connections."),
    ];
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Explore codebase") || has_label(&suggestions, "Run tests"),
        "code mentions fallback, got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_fallback_architecture_mentions() {
    let msgs = vec![
        make_user("overview"),
        make_assistant("The workspace has 17 crates with a modular architecture."),
    ];
    let suggestions = suggest_prompts(&msgs);
    assert!(
        has_label(&suggestions, "Show structure") || has_label(&suggestions, "Run cargo check"),
        "architecture fallback, got: {:?}",
        suggestions.iter().map(|s| &s.label).collect::<Vec<_>>()
    );
}

#[test]
fn test_generic_fallback() {
    let msgs = vec![
        make_user("what's up"),
        make_assistant("Not much. Just here."),
    ];
    let suggestions = suggest_prompts(&msgs);
    // Should get generic fallback suggestions.
    assert!(!suggestions.is_empty(), "should always have suggestions");
    assert!(suggestions.len() <= 3);
}

// ═══════════════════════════════════════════════════════════════════
// M. Tips Service — Session Lifecycle
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_tips_service_exhaustion_and_reset() {
    let mut svc = TipsService::new();
    let total = TipsService::total();
    assert!(total > 0);

    // Exhaust all tips.
    let mut seen_ids = std::collections::HashSet::new();
    for _ in 0..total {
        let tip = svc.next_tip().unwrap();
        seen_ids.insert(tip.id);
        svc.dismiss(tip.id);
    }

    // All tips seen, none remaining.
    assert_eq!(seen_ids.len(), total, "should see all unique tips");
    assert!(svc.next_tip().is_none());
    assert_eq!(svc.remaining(), 0);

    // Reset restores all tips.
    svc.reset();
    assert_eq!(svc.remaining(), total);
    assert!(svc.next_tip().is_some());
}

#[test]
fn test_tips_service_no_repeats_within_session() {
    let mut svc = TipsService::new();
    let total = TipsService::total();

    let mut seen = Vec::new();
    for _ in 0..total {
        let tip = svc.next_tip().unwrap();
        assert!(!seen.contains(&tip.id), "tip '{}' repeated!", tip.id);
        seen.push(tip.id);
        svc.dismiss(tip.id);
    }
}

#[test]
fn test_tips_service_partial_dismiss() {
    let mut svc = TipsService::new();
    let total = TipsService::total();

    // Dismiss first 3.
    for _ in 0..3 {
        let tip = svc.next_tip().unwrap();
        svc.dismiss(tip.id);
    }
    assert_eq!(svc.remaining(), total - 3);
}
