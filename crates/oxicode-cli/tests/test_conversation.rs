//! Integration tests for multi-turn conversation management.

use oxicode_common::{ContentBlock, Message};
use oxicode_core::Conversation;

#[test]
fn test_conversation_multi_turn_flow() {
    let mut conv = Conversation::new();
    assert!(conv.is_empty());

    let user1 = Message::user("What is Rust?");
    conv.push(user1);
    assert_eq!(conv.len(), 1);

    let mut assistant1 = Message::assistant();
    assistant1.content.push(ContentBlock::Text {
        text: "Rust is a systems programming language.".to_string(),
    });
    conv.push(assistant1);
    assert_eq!(conv.len(), 2);

    let user2 = Message::user("How does ownership work?");
    conv.push(user2);
    assert_eq!(conv.len(), 3);

    let api_msgs = conv.api_messages();
    assert_eq!(api_msgs.len(), 3);
    assert_eq!(api_msgs[0].text(), "What is Rust?");
    assert_eq!(
        api_msgs[1].text(),
        "Rust is a systems programming language."
    );
    assert_eq!(api_msgs[2].text(), "How does ownership work?");
}

#[test]
fn test_conversation_with_tool_use() {
    let mut conv = Conversation::new();

    conv.push(Message::user("Read the file main.rs"));

    let mut assistant = Message::assistant();
    assistant.content.push(ContentBlock::ToolUse {
        id: "tu_1".to_string(),
        name: "file_read".to_string(),
        input: serde_json::json!({"path": "src/main.rs"}),
    });
    conv.push(assistant);

    let mut tool_msg = Message::user("");
    tool_msg.content = vec![ContentBlock::ToolResult {
        tool_use_id: "tu_1".to_string(),
        content: "fn main() { println!(\"hello\"); }".to_string(),
        is_error: false,
    }];
    conv.push(tool_msg);

    assert_eq!(conv.len(), 3);
}

#[test]
fn test_conversation_replace_messages() {
    let mut conv = Conversation::new();
    conv.push(Message::user("first"));
    conv.push(Message::user("second"));
    assert_eq!(conv.len(), 2);

    conv.replace_messages(vec![Message::user("compacted")]);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.api_messages()[0].text(), "compacted");
}
