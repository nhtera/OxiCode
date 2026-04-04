//! Layer 1.5: Selective tool result removal (snip compact).
//!
//! Sits between L1 truncation and L2 microcompact. Removes tool result
//! content from old messages while preserving tool names for context.
//! Keeps first `KEEP_HEAD` and last `keep_last_n` tool results intact.

use oxicode_common::{ContentBlock, Message, Role};

/// Default number of recent tool results to preserve.
const DEFAULT_KEEP_LAST: usize = 10;

/// Number of earliest tool results to always preserve (context anchor).
const KEEP_HEAD: usize = 3;

/// Minimum conversation turns before a tool result is eligible for snipping.
const MIN_AGE_TURNS: usize = 5;

/// Config for snip compaction.
#[derive(Debug, Clone)]
pub struct SnipConfig {
    /// Number of most recent tool results to keep intact.
    pub keep_last_n: usize,
    /// Minimum turns old before a result can be snipped.
    pub min_age_turns: usize,
}

impl Default for SnipConfig {
    fn default() -> Self {
        Self {
            keep_last_n: DEFAULT_KEEP_LAST,
            min_age_turns: MIN_AGE_TURNS,
        }
    }
}

/// Result of a snip compaction pass.
#[derive(Debug, Default)]
pub struct SnipResult {
    /// Number of tool results snipped.
    pub snipped_count: usize,
    /// Approximate bytes freed.
    pub bytes_freed: usize,
}

/// Snip old tool result content from messages in-place.
///
/// Strategy:
/// 1. Find all ToolResult blocks across messages.
/// 2. Keep first KEEP_HEAD and last keep_last_n intact.
/// 3. Replace middle results with a compact placeholder.
/// 4. Only snip results older than min_age_turns from the end.
pub fn snip_compact(messages: &mut [Message], config: &SnipConfig) -> SnipResult {
    let mut result = SnipResult::default();

    // Collect positions of all ToolResult blocks: (msg_idx, content_idx, tool_name).
    let mut tool_positions: Vec<(usize, usize, String)> = Vec::new();

    for (msg_idx, msg) in messages.iter().enumerate() {
        if msg.role != Role::User {
            continue; // ToolResults are in user messages
        }
        for (block_idx, block) in msg.content.iter().enumerate() {
            if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                // Find matching tool name from previous assistant message.
                let tool_name = find_tool_name(messages, tool_use_id);
                tool_positions.push((msg_idx, block_idx, tool_name));
            }
        }
    }

    let total = tool_positions.len();
    if total <= KEEP_HEAD + config.keep_last_n {
        return result; // Nothing to snip.
    }

    // Determine age boundary: only snip results in messages older than min_age_turns.
    let age_boundary = messages.len().saturating_sub(config.min_age_turns);

    // Snip middle results (between KEEP_HEAD and total - keep_last_n).
    let snip_end = total.saturating_sub(config.keep_last_n);
    for &(msg_idx, block_idx, ref tool_name) in &tool_positions[KEEP_HEAD..snip_end] {
        if msg_idx >= age_boundary {
            continue; // Too recent to snip.
        }

        let msg = &mut messages[msg_idx];
        if let Some(ContentBlock::ToolResult { content, .. }) = msg.content.get_mut(block_idx) {
            let old_len = content.len();
            if old_len > 100 {
                // Only snip if content is substantial.
                let placeholder = format!(
                    "[Tool result snipped: {tool_name} — {old_len} bytes]"
                );
                result.bytes_freed += old_len.saturating_sub(placeholder.len());
                result.snipped_count += 1;
                *content = placeholder;
            }
        }
    }

    if result.snipped_count > 0 {
        tracing::info!(
            snipped = result.snipped_count,
            bytes_freed = result.bytes_freed,
            "L1.5: snip compact complete"
        );
    }

    result
}

/// Find the tool name for a given tool_use_id by scanning assistant messages.
fn find_tool_name(messages: &[Message], tool_use_id: &str) -> String {
    for msg in messages {
        if msg.role != Role::Assistant {
            continue;
        }
        for block in &msg.content {
            if let ContentBlock::ToolUse { id, name, .. } = block {
                if id == tool_use_id {
                    return name.clone();
                }
            }
        }
    }
    "unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool_conversation(n_tools: usize) -> Vec<Message> {
        let mut msgs = Vec::new();
        // First user message (context anchor).
        msgs.push(Message::user("Start the task."));

        for i in 0..n_tools {
            // Assistant uses a tool.
            let mut assistant = Message::assistant();
            assistant.content.push(ContentBlock::ToolUse {
                id: format!("tool_{i}"),
                name: format!("tool_{}", i % 3), // Rotate tool names
                input: serde_json::json!({}),
            });
            msgs.push(assistant);

            // User provides tool result.
            let mut user = Message::user("");
            user.content = vec![ContentBlock::ToolResult {
                tool_use_id: format!("tool_{i}"),
                content: format!("Result from tool {i}: {}", "x".repeat(200)),
                is_error: false,
            }];
            msgs.push(user);
        }

        // Final assistant response.
        let mut final_msg = Message::assistant();
        final_msg
            .content
            .push(ContentBlock::Text { text: "Done.".to_string() });
        msgs.push(final_msg);
        msgs
    }

    #[test]
    fn no_snip_when_few_tools() {
        let mut msgs = make_tool_conversation(5);
        let config = SnipConfig::default();
        let result = snip_compact(&mut msgs, &config);
        assert_eq!(result.snipped_count, 0);
    }

    #[test]
    fn snips_middle_tool_results() {
        let mut msgs = make_tool_conversation(25);
        let config = SnipConfig {
            keep_last_n: 5,
            min_age_turns: 3,
        };
        let result = snip_compact(&mut msgs, &config);
        assert!(result.snipped_count > 0, "should have snipped some results");
        assert!(result.bytes_freed > 0);
    }

    #[test]
    fn preserves_head_and_tail() {
        let mut msgs = make_tool_conversation(20);
        let config = SnipConfig {
            keep_last_n: 5,
            min_age_turns: 2,
        };
        snip_compact(&mut msgs, &config);

        // Check first KEEP_HEAD tool results are intact.
        let mut tool_count = 0;
        for msg in &msgs {
            for block in &msg.content {
                if let ContentBlock::ToolResult { content, .. } = block {
                    if tool_count < KEEP_HEAD {
                        assert!(
                            content.starts_with("Result from"),
                            "head result should be intact"
                        );
                    }
                    tool_count += 1;
                }
            }
        }
    }

    #[test]
    fn snipped_content_has_placeholder() {
        let mut msgs = make_tool_conversation(20);
        let config = SnipConfig {
            keep_last_n: 3,
            min_age_turns: 2,
        };
        snip_compact(&mut msgs, &config);

        let mut found_placeholder = false;
        for msg in &msgs {
            for block in &msg.content {
                if let ContentBlock::ToolResult { content, .. } = block {
                    if content.starts_with("[Tool result snipped:") {
                        found_placeholder = true;
                    }
                }
            }
        }
        assert!(found_placeholder, "should have placeholder text");
    }

    #[test]
    fn respects_min_age() {
        let mut msgs = make_tool_conversation(20);
        let config = SnipConfig {
            keep_last_n: 3,
            min_age_turns: 100, // Very high — nothing eligible
        };
        let result = snip_compact(&mut msgs, &config);
        assert_eq!(result.snipped_count, 0);
    }

    #[test]
    fn empty_messages_no_panic() {
        let mut msgs: Vec<Message> = Vec::new();
        let result = snip_compact(&mut msgs, &SnipConfig::default());
        assert_eq!(result.snipped_count, 0);
    }

    #[test]
    fn find_tool_name_works() {
        let msgs = make_tool_conversation(3);
        let name = find_tool_name(&msgs, "tool_0");
        assert_eq!(name, "tool_0");
    }

    #[test]
    fn find_tool_name_unknown() {
        let msgs = make_tool_conversation(1);
        let name = find_tool_name(&msgs, "nonexistent");
        assert_eq!(name, "unknown");
    }
}
