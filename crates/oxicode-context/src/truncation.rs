use oxicode_common::Message;

use crate::token_counter::TokenCounter;

/// Minimum number of most-recent messages always kept intact.
const DEFAULT_KEEP_LAST: usize = 10;

/// Layer-1 defense: remove oldest middle messages until token budget is met.
///
/// Strategy:
/// - Always keep the first message if it is a user message (context anchor).
/// - Always keep the last `keep_last` messages (active conversation tail).
/// - Remove messages from the middle (oldest first) until under budget.
/// - Returns a new `Vec`; input slice is not mutated.
pub fn truncate_messages(
    messages: &[Message],
    max_tokens: usize,
    counter: &mut TokenCounter,
) -> Vec<Message> {
    let total = counter.count_messages(messages);

    if total <= max_tokens {
        tracing::debug!(total, max_tokens, "L1: no truncation needed");
        return messages.to_vec();
    }

    tracing::info!(total, max_tokens, "L1: truncating messages");

    let keep_last = DEFAULT_KEEP_LAST.min(messages.len());
    let tail_start = messages.len().saturating_sub(keep_last);

    // Index 0 is always pinned (anchor); middle = [1..tail_start).
    // Build a removable list from the middle, oldest first.
    let mut removable: Vec<usize> = (1..tail_start).collect();

    let mut kept: Vec<bool> = vec![true; messages.len()];
    let mut current_tokens = total;

    while current_tokens > max_tokens && !removable.is_empty() {
        let idx = removable.remove(0); // oldest removable first
        let msg_tokens = counter.count_message(&messages[idx]);
        kept[idx] = false;
        current_tokens = current_tokens.saturating_sub(msg_tokens);
        tracing::debug!(idx, msg_tokens, current_tokens, "L1: dropped message");
    }

    let result: Vec<Message> = messages
        .iter()
        .enumerate()
        .filter(|(i, _)| kept[*i])
        .map(|(_, m)| m.clone())
        .collect();

    tracing::info!(
        before = messages.len(),
        after = result.len(),
        "L1: truncation complete"
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(texts: &[&str]) -> Vec<Message> {
        texts.iter().map(|t| Message::user(*t)).collect()
    }

    #[test]
    fn no_truncation_when_under_budget() {
        let msgs = make_messages(&["hello", "world"]);
        let mut counter = TokenCounter::new();
        let total = counter.count_messages(&msgs);
        let result = truncate_messages(&msgs, total + 100, &mut counter);
        assert_eq!(result.len(), msgs.len());
    }

    #[test]
    fn truncation_removes_middle_messages() {
        // Create enough messages so the middle can be dropped.
        let texts: Vec<String> = (0..15).map(|i| "a".repeat(40 * i.max(1))).collect();
        let text_refs: Vec<&str> = texts.iter().map(|s: &String| s.as_str()).collect();
        let msgs = make_messages(&text_refs);
        let mut counter = TokenCounter::new();
        let total = counter.count_messages(&msgs);
        // Force truncation by using a tight budget
        let budget = total / 2;
        let result = truncate_messages(&msgs, budget, &mut counter);
        assert!(result.len() < msgs.len(), "should have removed some messages");
        // First message always kept
        assert_eq!(result[0].id, msgs[0].id);
        // Last DEFAULT_KEEP_LAST messages kept
        let tail: Vec<&str> = msgs
            .iter()
            .rev()
            .take(DEFAULT_KEEP_LAST)
            .map(|m| m.id.as_str())
            .collect();
        let result_ids: Vec<&str> = result.iter().map(|m| m.id.as_str()).collect();
        for id in &tail {
            assert!(result_ids.contains(id), "tail message {id} should be kept");
        }
    }

    #[test]
    fn tiny_list_not_panics() {
        let msgs = make_messages(&["only one"]);
        let mut counter = TokenCounter::new();
        let result = truncate_messages(&msgs, 1, &mut counter);
        // Cannot drop below 1 message; returns what it can
        assert!(!result.is_empty());
    }
}
