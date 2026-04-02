/// Inter-agent messaging — mailbox-based message bus with concurrent access.
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

/// A message exchanged between named agents.
#[derive(Debug, Clone)]
pub struct AgentMessage {
    pub id: String,
    pub from: String,
    pub to: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

impl AgentMessage {
    pub fn new(from: impl Into<String>, to: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            from: from.into(),
            to: to.into(),
            content: content.into(),
            timestamp: Utc::now(),
        }
    }
}

/// Concurrent mailbox bus: each agent name maps to a queue of incoming messages.
pub struct MessageBus {
    mailboxes: RwLock<HashMap<String, Vec<AgentMessage>>>,
}

impl std::fmt::Debug for MessageBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageBus").finish()
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBus {
    pub fn new() -> Self {
        Self {
            mailboxes: RwLock::new(HashMap::new()),
        }
    }

    /// Deliver `msg` to its recipient's mailbox.
    pub async fn send(&self, msg: AgentMessage) {
        debug!(from = %msg.from, to = %msg.to, id = %msg.id, "bus: send message");
        let mut guard = self.mailboxes.write().await;
        guard.entry(msg.to.clone()).or_default().push(msg);
    }

    /// Drain and return all pending messages for `agent_name`.
    pub async fn receive(&self, agent_name: &str) -> Vec<AgentMessage> {
        let mut guard = self.mailboxes.write().await;
        let msgs = guard.remove(agent_name).unwrap_or_default();
        info!(agent = %agent_name, count = msgs.len(), "bus: receive messages");
        msgs
    }

    /// View pending messages for `agent_name` without consuming them.
    ///
    /// Returns a cloned snapshot because the `RwLock` guard cannot be held
    /// across an await boundary in the caller's context.
    pub async fn peek(&self, agent_name: &str) -> Vec<AgentMessage> {
        let guard = self.mailboxes.read().await;
        guard.get(agent_name).cloned().unwrap_or_default()
    }

    /// Returns the number of pending messages for `agent_name`.
    pub async fn pending_count(&self, agent_name: &str) -> usize {
        let guard = self.mailboxes.read().await;
        guard.get(agent_name).map_or(0, Vec::len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_send_and_receive() {
        let bus = MessageBus::new();
        let msg = AgentMessage::new("alice", "bob", "hello bob");

        bus.send(msg).await;

        let received = bus.receive("bob").await;
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].content, "hello bob");
        assert_eq!(received[0].from, "alice");
    }

    #[tokio::test]
    async fn test_receive_drains_mailbox() {
        let bus = MessageBus::new();
        bus.send(AgentMessage::new("a", "z", "msg1")).await;
        bus.send(AgentMessage::new("b", "z", "msg2")).await;

        let first = bus.receive("z").await;
        assert_eq!(first.len(), 2);

        // Mailbox is empty after drain.
        let second = bus.receive("z").await;
        assert!(second.is_empty());
    }

    #[tokio::test]
    async fn test_peek_does_not_consume() {
        let bus = MessageBus::new();
        bus.send(AgentMessage::new("x", "y", "peek me")).await;

        let peeked = bus.peek("y").await;
        assert_eq!(peeked.len(), 1);

        // Message still present after peek.
        let peeked_again = bus.peek("y").await;
        assert_eq!(peeked_again.len(), 1);
    }

    #[tokio::test]
    async fn test_pending_count() {
        let bus = MessageBus::new();
        assert_eq!(bus.pending_count("nobody").await, 0);
        bus.send(AgentMessage::new("a", "b", "hi")).await;
        assert_eq!(bus.pending_count("b").await, 1);
    }
}
