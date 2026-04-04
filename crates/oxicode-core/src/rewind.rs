//! Conversation rewind: remove the last N turn-pairs from a conversation.
//!
//! A "turn" is a user message plus its corresponding assistant response,
//! including any tool-use blocks interleaved between them.

use oxicode_common::{Message, Role};

/// Result of a rewind operation.
#[derive(Debug, Clone)]
pub struct RewindResult {
    /// Number of complete turns removed.
    pub turns_removed: usize,
    /// Number of individual messages removed.
    pub messages_removed: usize,
    /// Remaining messages after rewind.
    pub remaining: usize,
}

/// Rewind the last `turns` turn-pairs from a message list.
///
/// A turn is bounded by a user message at the start. We remove
/// `turns` user-message-initiated blocks from the end.
///
/// Returns `None` if messages is empty or turns is 0.
pub fn rewind(messages: &mut Vec<Message>, turns: usize) -> Option<RewindResult> {
    if turns == 0 || messages.is_empty() {
        return None;
    }

    let original_len = messages.len();

    // Walk backward, counting user messages as turn boundaries.
    let mut turns_found = 0;
    let mut cut_index = original_len;

    for i in (0..original_len).rev() {
        if messages[i].role == Role::User {
            turns_found += 1;
            cut_index = i;
            if turns_found >= turns {
                break;
            }
        }
    }

    // If we found no user messages, can't rewind meaningfully.
    if turns_found == 0 {
        return None;
    }

    let messages_removed = original_len - cut_index;
    messages.truncate(cut_index);

    Some(RewindResult {
        turns_removed: turns_found,
        messages_removed,
        remaining: cut_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_msg(text: &str) -> Message {
        Message::user(text)
    }

    fn assistant_msg(_text: &str) -> Message {
        // Message::assistant() creates an empty-content assistant message.
        // In tests we just need the role to be Assistant.
        Message::assistant()
    }

    #[test]
    fn rewind_zero_returns_none() {
        let mut msgs = vec![user_msg("hi"), assistant_msg("hello")];
        assert!(rewind(&mut msgs, 0).is_none());
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn rewind_empty_returns_none() {
        let mut msgs: Vec<Message> = vec![];
        assert!(rewind(&mut msgs, 1).is_none());
    }

    #[test]
    fn rewind_one_turn() {
        let mut msgs = vec![
            user_msg("first"),
            assistant_msg("reply1"),
            user_msg("second"),
            assistant_msg("reply2"),
        ];
        let result = rewind(&mut msgs, 1).unwrap();
        assert_eq!(result.turns_removed, 1);
        assert_eq!(result.messages_removed, 2);
        assert_eq!(result.remaining, 2);
        assert_eq!(msgs.len(), 2);
        // Only first turn remains.
        assert_eq!(msgs[0].role, Role::User);
        assert_eq!(msgs[1].role, Role::Assistant);
    }

    #[test]
    fn rewind_all_turns() {
        let mut msgs = vec![
            user_msg("first"),
            assistant_msg("reply1"),
            user_msg("second"),
            assistant_msg("reply2"),
        ];
        let result = rewind(&mut msgs, 5).unwrap(); // more than available
        assert_eq!(result.turns_removed, 2);
        assert_eq!(result.messages_removed, 4);
        assert_eq!(result.remaining, 0);
        assert!(msgs.is_empty());
    }

    #[test]
    fn rewind_no_user_messages() {
        let mut msgs = vec![assistant_msg("orphan")];
        assert!(rewind(&mut msgs, 1).is_none());
        assert_eq!(msgs.len(), 1); // unchanged
    }

    #[test]
    fn rewind_multiple_assistant_between_turns() {
        // Simulate: user -> assistant -> tool_result -> assistant (multi-step turn)
        let mut msgs = vec![
            user_msg("first"),
            assistant_msg("thinking..."),
            assistant_msg("final reply"),
            user_msg("second"),
            assistant_msg("reply2"),
        ];
        let result = rewind(&mut msgs, 1).unwrap();
        assert_eq!(result.turns_removed, 1);
        assert_eq!(result.messages_removed, 2); // "second" + "reply2"
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn rewind_preserves_first_turn_on_partial() {
        let mut msgs = vec![user_msg("only turn"), assistant_msg("only reply")];
        let result = rewind(&mut msgs, 1).unwrap();
        assert_eq!(result.turns_removed, 1);
        assert_eq!(result.remaining, 0);
        assert!(msgs.is_empty());
    }
}
