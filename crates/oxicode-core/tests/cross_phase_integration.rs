//! Cross-phase integration tests.
//!
//! Validates that features from different phases work together:
//! - Rewind + Conversation (Phase 8 + core)
//! - System prompt modes + assembly (Phase 8 + core)
//! - Rewind preserves conversation integrity
//! - System prompt ordering with all options combined

use oxicode_common::Message;
use oxicode_core::rewind;
use oxicode_core::system_prompt;
use oxicode_core::Conversation;

// ── Rewind + Conversation Integration ────────────────────────────

#[test]
fn rewind_then_continue_conversation() {
    let mut conv = Conversation::new();
    conv.push(Message::user("turn 1"));
    conv.push(Message::assistant());
    conv.push(Message::user("turn 2"));
    conv.push(Message::assistant());
    conv.push(Message::user("turn 3"));
    conv.push(Message::assistant());

    // Rewind 1 turn — removes turn 3 (user + assistant).
    let mut messages = conv.api_messages().to_vec();
    let result = rewind::rewind(&mut messages, 1).unwrap();
    assert_eq!(result.turns_removed, 1);
    assert_eq!(result.messages_removed, 2);

    // Replace conversation and add new turn.
    conv.replace_messages(messages);
    conv.push(Message::user("turn 3 replacement"));
    conv.push(Message::assistant());

    assert_eq!(conv.len(), 6); // 3 turns × 2 messages
    assert_eq!(conv.api_messages()[4].text(), "turn 3 replacement");
}

#[test]
fn rewind_all_then_rebuild() {
    let mut conv = Conversation::new();
    conv.push(Message::user("hello"));
    conv.push(Message::assistant());

    let mut messages = conv.api_messages().to_vec();
    let result = rewind::rewind(&mut messages, 100).unwrap();
    assert_eq!(result.remaining, 0);

    conv.replace_messages(messages);
    assert!(conv.is_empty());

    // Can rebuild from scratch.
    conv.push(Message::user("fresh start"));
    assert_eq!(conv.len(), 1);
}

// ── System Prompt Modes Integration ─────────────────────────────

#[test]
fn system_prompt_with_all_components_and_modes() {
    let skills = vec!["advisor_mode".to_string(), "sandbox_mode".to_string()];
    let prompt = system_prompt::assemble_system_prompt_with_modes(
        Some("global rules"),
        Some("project rules"),
        Some("skill definitions"),
        Some("memory facts"),
        None,
        &skills,
    );

    // All sections present.
    assert!(prompt.contains("OxiCode"));
    assert!(prompt.contains("Active Modes"));
    assert!(prompt.contains("advisor mode"));
    assert!(prompt.contains("Sandbox mode"));
    assert!(prompt.contains("global rules"));
    assert!(prompt.contains("project rules"));
    assert!(prompt.contains("memory facts"));
    assert!(prompt.contains("skill definitions"));

    // Verify ordering: base < modes < global < project < memory < skills.
    let base_pos = prompt.find("OxiCode").unwrap();
    let modes_pos = prompt.find("Active Modes").unwrap();
    let global_pos = prompt.find("Global Instructions").unwrap();
    let project_pos = prompt.find("Project Instructions").unwrap();
    let memory_pos = prompt.find("Project Memory").unwrap();
    let skills_pos = prompt.find("Active Skills").unwrap();

    assert!(base_pos < modes_pos);
    assert!(modes_pos < global_pos);
    assert!(global_pos < project_pos);
    assert!(project_pos < memory_pos);
    assert!(memory_pos < skills_pos);
}

#[test]
fn mode_injection_text_appends_to_existing_prompt() {
    let base = system_prompt::assemble_system_prompt(Some("global"), Some("project"), None, None);
    let skills = vec!["advisor_mode".to_string()];
    let injection = system_prompt::mode_injection_text(&skills).unwrap();

    let combined = format!("{base}{injection}");
    assert!(combined.contains("OxiCode"));
    assert!(combined.contains("global"));
    assert!(combined.contains("advisor mode"));
}

#[test]
fn no_modes_no_injection() {
    let skills = vec!["extended_thinking".to_string()];
    assert!(system_prompt::mode_injection_text(&skills).is_none());
}

// ── Conversation State Integrity ────────────────────────────────

#[test]
fn conversation_replace_preserves_api_contract() {
    let mut conv = Conversation::new();
    for i in 0..10 {
        conv.push(Message::user(format!("msg {i}")));
        conv.push(Message::assistant());
    }
    assert_eq!(conv.len(), 20);

    // Simulate compaction: replace with subset.
    let compacted = conv.api_messages()[14..].to_vec();
    conv.replace_messages(compacted);
    assert_eq!(conv.len(), 6);
    assert_eq!(conv.api_messages().len(), 6);
}
