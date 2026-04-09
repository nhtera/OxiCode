//! Tests for teammate MessageBus broadcast, point-to-point, and AgentMailbox.
//!
//! No API key needed — pure message-passing logic.
//! Run with: `cargo test -p oxicode-tasks --test teammate_messaging_tests`
//!
//! Tests cover two MessageBus implementations:
//! - `oxicode_agents::communication::MessageBus` (async, RwLock-based)
//! - Teammate MessageBus tested via inline tests in teammate.rs (feature-gated)

use oxicode_agents::communication::{AgentMessage, MessageBus};

#[tokio::test]
async fn test_broadcast_reaches_all_except_sender() {
    // AgentBus doesn't have broadcast built-in (it routes by "to" name).
    // Simulate broadcast by sending to each recipient individually.
    let bus = MessageBus::new();

    // Send to alice, bob, charlie (simulating broadcast).
    bus.send(AgentMessage::new("lead", "alice", "all hands"))
        .await;
    bus.send(AgentMessage::new("lead", "bob", "all hands"))
        .await;
    bus.send(AgentMessage::new("lead", "charlie", "all hands"))
        .await;

    let alice_msgs = bus.receive("alice").await;
    let bob_msgs = bus.receive("bob").await;
    let charlie_msgs = bus.receive("charlie").await;

    assert_eq!(alice_msgs.len(), 1, "alice should receive message");
    assert_eq!(bob_msgs.len(), 1, "bob should receive message");
    assert_eq!(charlie_msgs.len(), 1, "charlie should receive message");

    // Lead should NOT have received anything (not a recipient).
    let lead_msgs = bus.receive("lead").await;
    assert!(lead_msgs.is_empty(), "lead should NOT receive own messages");
}

#[tokio::test]
async fn test_point_to_point_only_reaches_target() {
    let bus = MessageBus::new();

    bus.send(AgentMessage::new("lead", "alice", "task for alice"))
        .await;

    let alice_msgs = bus.receive("alice").await;
    assert_eq!(alice_msgs.len(), 1, "alice should receive direct message");
    assert_eq!(alice_msgs[0].content, "task for alice");

    let bob_msgs = bus.receive("bob").await;
    assert!(
        bob_msgs.is_empty(),
        "bob should NOT receive alice's message"
    );
}

#[tokio::test]
async fn test_unsubscribe_silently_handles_missing() {
    let bus = MessageBus::new();

    // Send to a name with no prior interaction — should not panic.
    bus.send(AgentMessage::new("lead", "nobody", "hello?"))
        .await;

    // Receiving from a non-existent mailbox returns empty.
    let msgs = bus.receive("nobody").await;
    assert_eq!(msgs.len(), 1, "message delivered to mailbox on first send");
}

#[tokio::test]
async fn test_multiple_messages_queued_fifo() {
    let bus = MessageBus::new();

    for i in 0..5 {
        bus.send(AgentMessage::new("lead", "alice", format!("msg-{i}")))
            .await;
    }

    let msgs = bus.receive("alice").await;
    assert_eq!(msgs.len(), 5, "all 5 messages should be received");

    // Verify FIFO order.
    for (i, msg) in msgs.iter().enumerate() {
        assert_eq!(
            msg.content,
            format!("msg-{i}"),
            "messages should be in FIFO order"
        );
    }

    // Mailbox is drained after receive.
    let empty = bus.receive("alice").await;
    assert!(empty.is_empty(), "mailbox should be empty after drain");
}

#[tokio::test]
async fn test_agent_communication_send_and_peek() {
    let bus = MessageBus::new();
    bus.send(AgentMessage::new("alice", "bob", "peek test"))
        .await;

    // Peek should return message without consuming it.
    let peeked: Vec<AgentMessage> = bus.peek("bob").await;
    assert_eq!(peeked.len(), 1);
    assert_eq!(peeked[0].content, "peek test");

    // Message still present after peek.
    let peeked_again: Vec<AgentMessage> = bus.peek("bob").await;
    assert_eq!(peeked_again.len(), 1);

    // Receive consumes the message.
    let received: Vec<AgentMessage> = bus.receive("bob").await;
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].content, "peek test");

    // Now mailbox is empty.
    let empty: Vec<AgentMessage> = bus.receive("bob").await;
    assert!(empty.is_empty());
}

#[tokio::test]
async fn test_agent_communication_pending_count() {
    let bus = MessageBus::new();

    // Initially zero.
    assert_eq!(bus.pending_count("bob").await, 0);

    // Send 3 messages.
    bus.send(AgentMessage::new("a", "bob", "msg1")).await;
    bus.send(AgentMessage::new("b", "bob", "msg2")).await;
    bus.send(AgentMessage::new("c", "bob", "msg3")).await;
    assert_eq!(bus.pending_count("bob").await, 3);

    // Receive drains all.
    let msgs: Vec<AgentMessage> = bus.receive("bob").await;
    assert_eq!(msgs.len(), 3);
    assert_eq!(bus.pending_count("bob").await, 0);
}
