use oxicode_common::Message;
use serde::{Deserialize, Serialize};

/// A conversation is an ordered list of messages with metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Conversation {
    pub messages: Vec<Message>,
}

impl Conversation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Get all messages suitable for sending to the API (non-system).
    pub fn api_messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Replace all messages (used after context compaction).
    pub fn replace_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_is_empty() {
        let conv = Conversation::new();
        assert!(conv.is_empty());
        assert_eq!(conv.len(), 0);
    }

    #[test]
    fn test_push_and_len() {
        let mut conv = Conversation::new();
        conv.push(Message::user("hello"));
        assert!(!conv.is_empty());
        assert_eq!(conv.len(), 1);
    }

    #[test]
    fn test_api_messages() {
        let mut conv = Conversation::new();
        conv.push(Message::user("hello"));
        conv.push(Message::assistant());
        assert_eq!(conv.api_messages().len(), 2);
    }

    #[test]
    fn test_replace_messages() {
        let mut conv = Conversation::new();
        conv.push(Message::user("one"));
        conv.push(Message::user("two"));
        conv.push(Message::user("three"));
        assert_eq!(conv.len(), 3);

        conv.replace_messages(vec![Message::user("replaced")]);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.api_messages()[0].text(), "replaced");
    }

    #[test]
    fn test_replace_with_empty() {
        let mut conv = Conversation::new();
        conv.push(Message::user("hello"));
        conv.replace_messages(vec![]);
        assert!(conv.is_empty());
    }

    #[test]
    fn test_default_is_empty() {
        let conv = Conversation::default();
        assert!(conv.is_empty());
    }
}
