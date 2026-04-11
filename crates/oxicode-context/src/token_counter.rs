use std::collections::HashMap;

use oxicode_common::{ContentBlock, Message};

/// Heuristic token counter (chars/4 + per-message overhead).
/// Does NOT use tiktoken — compatible with all platforms.
#[derive(Debug, Clone, Default)]
pub struct TokenCounter {
    /// Cache: message ID → estimated token count.
    cache: HashMap<String, usize>,
}

impl TokenCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Estimate tokens in a single text string (chars / 4).
    pub fn count_text(text: &str) -> usize {
        text.len() / 4
    }

    /// Estimate tokens for one message, using cache when available.
    pub fn count_message(&mut self, msg: &Message) -> usize {
        if let Some(&cached) = self.cache.get(&msg.id) {
            return cached;
        }

        // 4-token framing overhead per message (role + structural tokens).
        let mut tokens: usize = 4;

        for block in &msg.content {
            tokens += count_block(block);
        }

        self.cache.insert(msg.id.clone(), tokens);
        tracing::debug!(msg_id = %msg.id, tokens, "counted message tokens");
        tokens
    }

    /// Total tokens across a slice of messages.
    pub fn count_messages(&mut self, msgs: &[Message]) -> usize {
        msgs.iter().map(|m| self.count_message(m)).sum()
    }

    /// Drop the cached entry for a message (call after mutation).
    pub fn invalidate(&mut self, msg_id: &str) {
        self.cache.remove(msg_id);
    }

    /// Clear the entire cache.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}

/// Estimate tokens for one content block.
fn count_block(block: &ContentBlock) -> usize {
    match block {
        ContentBlock::Text { text } => TokenCounter::count_text(text),
        ContentBlock::Image { .. } => {
            // Anthropic uses ~1600 tokens for a typical image (varies by resolution).
            1600
        }
        ContentBlock::ToolUse { name, input, .. } => {
            let input_str = input.to_string();
            TokenCounter::count_text(name) + TokenCounter::count_text(&input_str)
        }
        ContentBlock::ToolResult { content, .. } => TokenCounter::count_text(content),
        ContentBlock::Thinking { thinking } => TokenCounter::count_text(thinking),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_text_basic() {
        // 40 chars → ~10 tokens
        let text = "a".repeat(40);
        assert_eq!(TokenCounter::count_text(&text), 10);
    }

    #[test]
    fn count_message_overhead() {
        let msg = Message::user("aaaa"); // 4 chars = 1 token + 4 overhead = 5
        let mut counter = TokenCounter::new();
        assert_eq!(counter.count_message(&msg), 5);
    }

    #[test]
    fn count_message_cached() {
        let msg = Message::user("hello world test");
        let mut counter = TokenCounter::new();
        let first = counter.count_message(&msg);
        let second = counter.count_message(&msg);
        assert_eq!(first, second);
        assert_eq!(counter.cache.len(), 1);
    }

    #[test]
    fn count_messages_sums_correctly() {
        let msgs = vec![Message::user("aaaa"), Message::user("bbbbbbbb")];
        let mut counter = TokenCounter::new();
        // msg1: 4/4=1 + 4 = 5; msg2: 8/4=2 + 4 = 6 → total 11
        assert_eq!(counter.count_messages(&msgs), 11);
    }

    #[test]
    fn count_tool_use_block() {
        let mut msg = Message::user("");
        msg.content = vec![ContentBlock::ToolUse {
            id: "id1".to_string(),
            name: "read_file".to_string(), // 9 chars = 2 tokens
            input: serde_json::json!({"path": "/tmp"}), // ~16 chars = 4 tokens
        }];
        let mut counter = TokenCounter::new();
        // 4 overhead + name(2) + input(varies) — just check > 4
        assert!(counter.count_message(&msg) > 4);
    }
}
