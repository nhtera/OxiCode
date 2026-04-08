//! Post-compact cleanup: restore critical context after any compaction.
//!
//! After compaction removes messages, key context (working directory,
//! recent tool names, active skills) may be lost. This module re-injects
//! a compact context-restoration message capped at 2K tokens.

use oxicode_common::{ContentBlock, Message, Role};

/// Max approximate tokens for the restoration message.
const MAX_RESTORE_TOKENS: usize = 2_000;

/// Approximate chars-per-token for budget estimation.
const CHARS_PER_TOKEN: usize = 4;

/// Max chars for restoration content (~2K tokens).
const MAX_RESTORE_CHARS: usize = MAX_RESTORE_TOKENS * CHARS_PER_TOKEN;

/// Context to restore after compaction.
#[derive(Debug, Clone, Default)]
pub struct RestoreContext {
    /// Current working directory.
    pub working_dir: Option<String>,
    /// Recently used tool names (last N turns).
    pub recent_tools: Vec<String>,
    /// Active skill names or prompts.
    pub active_skills: Vec<String>,
    /// Project memory snippet (from MEMORY.md).
    pub memory_snippet: Option<String>,
}

/// Result of a post-compact restore.
#[derive(Debug, Default)]
pub struct RestoreResult {
    /// Whether a restoration message was injected.
    pub restored: bool,
    /// Approximate tokens used by the restoration.
    pub tokens_used: usize,
}

/// Build a restoration message from the given context.
/// Returns None if there's nothing meaningful to restore.
pub fn build_restore_message(ctx: &RestoreContext) -> Option<Message> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(dir) = &ctx.working_dir {
        parts.push(format!("Working directory: {dir}"));
    }

    if !ctx.recent_tools.is_empty() {
        let tools = ctx.recent_tools.join(", ");
        parts.push(format!("Recent tools: {tools}"));
    }

    if !ctx.active_skills.is_empty() {
        let skills = ctx.active_skills.join(", ");
        parts.push(format!("Active skills: {skills}"));
    }

    if let Some(memory) = &ctx.memory_snippet {
        if !memory.is_empty() {
            parts.push(format!("Project memory:\n{memory}"));
        }
    }

    if parts.is_empty() {
        return None;
    }

    let mut content = "[Context restored after compaction]\n".to_string();
    for part in &parts {
        content.push_str(part);
        content.push('\n');
    }

    // Enforce byte cap (snap to char boundary to avoid panicking on multi-byte UTF-8).
    if content.len() > MAX_RESTORE_CHARS {
        let mut end = MAX_RESTORE_CHARS;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        content.truncate(end);
        // Find last newline to avoid cutting mid-line.
        if let Some(pos) = content.rfind('\n') {
            content.truncate(pos);
        }
        content.push_str("\n[... truncated]");
    }

    Some(Message::user(content))
}

/// Inject a context restoration message into messages after compaction.
/// Appends the restoration as the last user message before the final assistant turn.
pub fn post_compact_restore(messages: &mut Vec<Message>, ctx: &RestoreContext) -> RestoreResult {
    let Some(restore_msg) = build_restore_message(ctx) else {
        return RestoreResult::default();
    };

    let tokens_used = restore_msg
        .content
        .iter()
        .map(|b| match b {
            ContentBlock::Text { text } => text.len() / CHARS_PER_TOKEN,
            _ => 0,
        })
        .sum();

    // Insert before the last assistant message, or append at end.
    let insert_pos = messages
        .iter()
        .rposition(|m| m.role == Role::Assistant)
        .unwrap_or(messages.len());

    messages.insert(insert_pos, restore_msg);

    tracing::info!(tokens_used, "post-compact: context restored");

    RestoreResult {
        restored: true,
        tokens_used,
    }
}

/// Extract recent tool names from messages (last N turns).
pub fn extract_recent_tools(messages: &[Message], last_n_turns: usize) -> Vec<String> {
    let mut tools = Vec::new();
    let start = messages.len().saturating_sub(last_n_turns * 2);

    for msg in &messages[start..] {
        if msg.role != Role::Assistant {
            continue;
        }
        for block in &msg.content {
            if let ContentBlock::ToolUse { name, .. } = block {
                if !tools.contains(name) {
                    tools.push(name.clone());
                }
            }
        }
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_context_returns_none() {
        let ctx = RestoreContext::default();
        assert!(build_restore_message(&ctx).is_none());
    }

    #[test]
    fn builds_message_with_working_dir() {
        let ctx = RestoreContext {
            working_dir: Some("/home/user/project".to_string()),
            ..Default::default()
        };
        let msg = build_restore_message(&ctx).unwrap();
        let text = msg.content[0].as_text().unwrap();
        assert!(text.contains("/home/user/project"));
        assert!(text.contains("Context restored"));
    }

    #[test]
    fn builds_message_with_tools() {
        let ctx = RestoreContext {
            recent_tools: vec!["Read".to_string(), "Edit".to_string()],
            ..Default::default()
        };
        let msg = build_restore_message(&ctx).unwrap();
        let text = msg.content[0].as_text().unwrap();
        assert!(text.contains("Read, Edit"));
    }

    #[test]
    fn builds_message_with_all_fields() {
        let ctx = RestoreContext {
            working_dir: Some("/project".to_string()),
            recent_tools: vec!["Bash".to_string()],
            active_skills: vec!["cook".to_string()],
            memory_snippet: Some("Use Rust".to_string()),
        };
        let msg = build_restore_message(&ctx).unwrap();
        let text = msg.content[0].as_text().unwrap();
        assert!(text.contains("/project"));
        assert!(text.contains("Bash"));
        assert!(text.contains("cook"));
        assert!(text.contains("Use Rust"));
    }

    #[test]
    fn truncates_oversized_content() {
        let ctx = RestoreContext {
            memory_snippet: Some("x".repeat(MAX_RESTORE_CHARS + 1000)),
            ..Default::default()
        };
        let msg = build_restore_message(&ctx).unwrap();
        let text = msg.content[0].as_text().unwrap();
        assert!(text.len() <= MAX_RESTORE_CHARS + 50); // Allow for truncation marker
        assert!(text.contains("truncated"));
    }

    #[test]
    fn post_compact_restore_injects_message() {
        let mut msgs = vec![Message::user("Hello"), Message::assistant()];
        let ctx = RestoreContext {
            working_dir: Some("/test".to_string()),
            ..Default::default()
        };
        let result = post_compact_restore(&mut msgs, &ctx);
        assert!(result.restored);
        assert_eq!(msgs.len(), 3);
        // Restoration should be before the assistant message.
        assert_eq!(msgs[1].role, Role::User);
    }

    #[test]
    fn post_compact_restore_skips_empty_context() {
        let mut msgs = vec![Message::user("Hello")];
        let ctx = RestoreContext::default();
        let result = post_compact_restore(&mut msgs, &ctx);
        assert!(!result.restored);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn extract_recent_tools_finds_tools() {
        let mut assistant = Message::assistant();
        assistant.content.push(ContentBlock::ToolUse {
            id: "t1".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({}),
        });
        assistant.content.push(ContentBlock::ToolUse {
            id: "t2".to_string(),
            name: "Edit".to_string(),
            input: serde_json::json!({}),
        });

        let msgs = vec![Message::user("do stuff"), assistant];
        let tools = extract_recent_tools(&msgs, 5);
        assert_eq!(tools, vec!["Read", "Edit"]);
    }

    #[test]
    fn extract_recent_tools_deduplicates() {
        let mut a1 = Message::assistant();
        a1.content.push(ContentBlock::ToolUse {
            id: "t1".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({}),
        });
        let mut a2 = Message::assistant();
        a2.content.push(ContentBlock::ToolUse {
            id: "t2".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({}),
        });

        let msgs = vec![Message::user("a"), a1, Message::user("b"), a2];
        let tools = extract_recent_tools(&msgs, 10);
        assert_eq!(tools, vec!["Read"]); // No duplicates
    }
}
