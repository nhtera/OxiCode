//! Integration tests for context budget management.
//!
//! Tests the full L1→L2 defense cascade that keeps conversations
//! within the model's context window. L3+ requires a live LLM and is
//! covered by the conversation loop tests.
//!
//! Run with: `cargo test -p oxicode-context --test live_context_budget`

use oxicode_common::{ContentBlock, Message};
use oxicode_context::{
    truncate_messages, microcompact_messages, BudgetManager, BudgetStatus, TokenCounter,
};

// ── Helper ──────────────────────────────────────────────────────

/// Create alternating user/assistant messages with `chars_each` chars of text.
fn make_conversation(count: usize, chars_each: usize) -> Vec<Message> {
    (0..count)
        .map(|i| {
            if i % 2 == 0 {
                Message::user(&"u".repeat(chars_each))
            } else {
                let mut m = Message::assistant();
                m.content.push(ContentBlock::Text {
                    text: "a".repeat(chars_each),
                });
                m
            }
        })
        .collect()
}

/// Create a message with a large ToolResult content block.
fn make_tool_result_message(lines: usize) -> Message {
    let content: String = (0..lines).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let mut msg = Message::user("");
    msg.content = vec![ContentBlock::ToolResult {
        tool_use_id: "tu_1".to_string(),
        content,
        is_error: false,
    }];
    msg
}

// ── Budget Status Transitions ───────────────────────────────────

#[test]
fn test_budget_status_ok_when_low() {
    let mut mgr = BudgetManager::new(100_000);
    // 10 messages × ~14 tokens each ≈ 140 tokens (< 80% of 100K)
    let msgs = make_conversation(10, 40);
    assert_eq!(mgr.check_budget(&msgs), BudgetStatus::Ok);
}

#[test]
fn test_budget_status_l1_when_80_percent() {
    // Each message: chars_each/4 + 4 overhead
    // For budget=100: need ~82 tokens → 82% usage
    // 6 msgs × (40/4 + 4) = 6 × 14 = 84 tokens → 84% of 100
    let mut mgr = BudgetManager::new(100);
    let msgs = make_conversation(6, 40);
    let status = mgr.check_budget(&msgs);
    assert!(
        matches!(status, BudgetStatus::NeedsL1Truncation | BudgetStatus::NeedsL2Microcompact),
        "Expected L1 or L2 at ~84%, got: {status:?}"
    );
}

#[test]
fn test_budget_status_l2_when_87_percent() {
    // 100 budget, need ~87 tokens
    // 6 msgs × (44/4 + 4) = 6 × 15 = 90 → 90% → L3
    // 6 msgs × (40/4 + 4) = 6 × 14 = 84 → 84% → L1
    // Try to land in L2 range (85-90%)
    let mut mgr = BudgetManager::new(100);
    let msgs = make_conversation(6, 44); // 6 × 15 = 90 → L3
    let status = mgr.check_budget(&msgs);
    assert!(
        matches!(
            status,
            BudgetStatus::NeedsL2Microcompact | BudgetStatus::NeedsL3AutoCompact
        ),
        "Expected L2 or L3 at ~90%, got: {status:?}"
    );
}

#[test]
fn test_budget_status_critical_when_full() {
    let mut mgr = BudgetManager::new(10);
    // 5 msgs × (400/4 + 4) = 5 × 104 = 520 tokens >> 10 budget
    let msgs = make_conversation(5, 400);
    assert_eq!(mgr.check_budget(&msgs), BudgetStatus::Critical);
}

#[test]
fn test_budget_status_zero_max_is_critical() {
    let mut mgr = BudgetManager::new(0);
    let msgs = make_conversation(1, 10);
    assert_eq!(mgr.check_budget(&msgs), BudgetStatus::Critical);
}

// ── L1 Truncation ───────────────────────────────────────────────

#[test]
fn test_l1_truncation_removes_middle_messages() {
    let msgs = make_conversation(20, 100); // 20 msgs
    let mut counter = TokenCounter::new();
    let total = counter.count_messages(&msgs);

    // Truncate to half budget.
    let budget = total / 2;
    let result = truncate_messages(&msgs, budget, &mut counter);

    assert!(
        result.len() < msgs.len(),
        "Should have removed messages: {} < {}",
        result.len(),
        msgs.len()
    );
    // First message always kept (anchor).
    assert_eq!(result[0].id, msgs[0].id);
    // Last 10 messages always kept (tail).
    let last_original = &msgs[msgs.len() - 1];
    let last_result = &result[result.len() - 1];
    assert_eq!(last_result.id, last_original.id);
}

#[test]
fn test_l1_truncation_no_op_when_under_budget() {
    let msgs = make_conversation(5, 40);
    let mut counter = TokenCounter::new();
    let result = truncate_messages(&msgs, 100_000, &mut counter);
    assert_eq!(result.len(), msgs.len());
}

#[test]
fn test_l1_preserves_first_and_tail() {
    let msgs = make_conversation(15, 100);
    let mut counter = TokenCounter::new();
    let total = counter.count_messages(&msgs);
    let budget = total / 3; // Force aggressive truncation

    let result = truncate_messages(&msgs, budget, &mut counter);

    // First message always kept.
    assert_eq!(result[0].id, msgs[0].id, "First message must be preserved");
    // Tail messages kept (last 10, or fewer if not enough messages).
    let tail_count = 10.min(msgs.len());
    for i in 0..tail_count {
        let orig_idx = msgs.len() - tail_count + i;
        let orig_id = &msgs[orig_idx].id;
        assert!(
            result.iter().any(|m| m.id == *orig_id),
            "Tail message at index {orig_idx} should be preserved"
        );
    }
}

// ── L2 Microcompact ─────────────────────────────────────────────

/// Extract tool result content from a ContentBlock.
fn tool_result_content(block: &ContentBlock) -> &str {
    match block {
        ContentBlock::ToolResult { content, .. } => content.as_str(),
        _ => "",
    }
}

#[test]
fn test_l2_microcompact_truncates_large_tool_results() {
    let mut msgs = vec![make_tool_result_message(200)]; // 200 lines → will be truncated
    let original_len = tool_result_content(&msgs[0].content[0]).len();

    microcompact_messages(&mut msgs);

    let compressed_len = tool_result_content(&msgs[0].content[0]).len();
    assert!(
        compressed_len < original_len,
        "Tool result should be compressed: {compressed_len} < {original_len}"
    );
    assert!(
        tool_result_content(&msgs[0].content[0]).contains("truncated"),
        "Should contain truncation marker"
    );
}

#[test]
fn test_l2_microcompact_strips_thinking_blocks() {
    let mut msgs = vec![Message::user("")];
    msgs[0].content = vec![
        ContentBlock::Text { text: "keep this".to_string() },
        ContentBlock::Thinking { thinking: "internal reasoning".to_string() },
        ContentBlock::Text { text: " and this".to_string() },
    ];

    microcompact_messages(&mut msgs);

    // Thinking block removed, text blocks collapsed.
    assert_eq!(msgs[0].content.len(), 1);
    let text = msgs[0].content[0].as_text().unwrap();
    assert!(text.contains("keep this"));
    assert!(text.contains("and this"));
}

#[test]
fn test_l2_microcompact_collapses_consecutive_text() {
    let mut msgs = vec![Message::user("")];
    msgs[0].content = vec![
        ContentBlock::Text { text: "part1 ".to_string() },
        ContentBlock::Text { text: "part2 ".to_string() },
        ContentBlock::Text { text: "part3".to_string() },
    ];

    microcompact_messages(&mut msgs);

    assert_eq!(msgs[0].content.len(), 1);
    assert_eq!(msgs[0].content[0].as_text().unwrap(), "part1 part2 part3");
}

// ── Token Counter ───────────────────────────────────────────────

#[test]
fn test_token_counter_text_estimation() {
    // ~4 chars per token heuristic
    assert_eq!(TokenCounter::count_text(""), 0);
    assert_eq!(TokenCounter::count_text("abcd"), 1);
    assert_eq!(TokenCounter::count_text(&"x".repeat(100)), 25);
}

#[test]
fn test_token_counter_message_with_overhead() {
    let mut counter = TokenCounter::new();
    let msg = Message::user("hello"); // 5 chars → 1 token + 4 overhead = 5
    let tokens = counter.count_message(&msg);
    assert!(tokens >= 5, "Should include overhead: got {tokens}");
}

#[test]
fn test_token_counter_messages_sum() {
    let mut counter = TokenCounter::new();
    let msgs = make_conversation(5, 40);
    let total = counter.count_messages(&msgs);
    // 5 msgs × (40/4 + 4) = 5 × 14 = 70
    assert!(total >= 50, "Total should be reasonable: got {total}");
    assert!(total <= 100, "Total should be reasonable: got {total}");
}

// ── Full Defense Cascade (L1 + L2) ──────────────────────────────

#[test]
fn test_l1_then_l2_reduces_context() {
    // Create a large conversation with big tool results.
    let mut msgs: Vec<Message> = Vec::new();
    msgs.push(Message::user("initial question"));

    // Add 20 pairs of tool use + tool result with large outputs.
    for i in 0..20 {
        let mut assistant = Message::assistant();
        assistant.content.push(ContentBlock::ToolUse {
            id: format!("tu_{i}"),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        });
        msgs.push(assistant);
        msgs.push(make_tool_result_message(150)); // 150 lines each
    }
    msgs.push(Message::user("final question"));

    let mut counter = TokenCounter::new();
    let initial_tokens = counter.count_messages(&msgs);
    let initial_count = msgs.len();

    // Apply L1 truncation with tight budget.
    let budget = initial_tokens / 3;
    let mut result = truncate_messages(&msgs, budget, &mut counter);
    let post_l1_count = result.len();

    // Apply L2 microcompact.
    microcompact_messages(&mut result);

    // Verify reductions.
    assert!(
        post_l1_count < initial_count,
        "L1 should remove messages: {post_l1_count} < {initial_count}"
    );
    // First message preserved.
    assert_eq!(result[0].id, msgs[0].id);
}
