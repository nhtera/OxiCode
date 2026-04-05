//! Integration tests for the hooks system.
//!
//! Tests hook lifecycle: config loading → event firing → response handling.
//! These tests execute real shell subprocesses (no API required).
//!
//! Run with: `cargo test -p oxicode-hooks --test live_hooks_execution`

use oxicode_hooks::config::{HookDef, HookType, HooksConfig};
use oxicode_hooks::events::{HookEvent, HookPayload, HookResponse};
use oxicode_hooks::executor::{execute_hook, execute_hook_script};
use oxicode_hooks::manager::HookManager;

use std::time::Duration;

// ── Helper ──────────────────────────────────────────────────────

fn make_payload(event: HookEvent) -> HookPayload {
    HookPayload {
        event,
        data: serde_json::json!({"tool": "bash", "command": "echo test"}),
        session_id: Some("test-session-123".to_string()),
        model: Some("claude-sonnet-4".to_string()),
    }
}

fn make_hook_def(command: &str) -> HookDef {
    HookDef {
        hook_type: HookType::Command,
        command: command.to_string(),
        timeout_secs: 5,
        enabled: true,
        agent: None,
        http: None,
        instructions: None,
        model: None,
        url: None,
        authorization: None,
    }
}

// ── HookManager ─────────────────────────────────────────────────

#[tokio::test]
async fn test_noop_manager_passes_all_events() {
    let mgr = HookManager::noop();

    for event in HookEvent::ALL {
        let response = mgr.fire(*event, serde_json::json!({})).await;
        assert!(
            matches!(response, HookResponse::Pass),
            "Noop manager should Pass for {event:?}, got: {response:?}"
        );
    }
}

#[tokio::test]
async fn test_manager_has_hook_returns_false_for_noop() {
    let mgr = HookManager::noop();
    assert!(!mgr.has_hook(HookEvent::SessionStart));
    assert!(!mgr.has_hook(HookEvent::ToolCallBefore));
}

// ── Command Hook Execution ──────────────────────────────────────

#[tokio::test]
async fn test_command_hook_pass_response() {
    let response = execute_hook_script(
        r#"echo '{"action":"pass"}'"#,
        &make_payload(HookEvent::SessionStart),
        Some(Duration::from_secs(5)),
    )
    .await;
    assert!(matches!(response, HookResponse::Pass));
}

#[tokio::test]
async fn test_command_hook_modify_prompt_response() {
    let response = execute_hook_script(
        r#"echo '{"action":"modify_prompt","text":"extra instructions"}'"#,
        &make_payload(HookEvent::PreQuery),
        Some(Duration::from_secs(5)),
    )
    .await;
    assert!(
        matches!(&response, HookResponse::ModifyPrompt { text } if text == "extra instructions"),
        "Expected ModifyPrompt, got: {response:?}"
    );
}

#[tokio::test]
async fn test_command_hook_abort_response() {
    let response = execute_hook_script(
        r#"echo '{"action":"abort","reason":"blocked by security policy"}'"#,
        &make_payload(HookEvent::ToolCallBefore),
        Some(Duration::from_secs(5)),
    )
    .await;
    assert!(
        matches!(&response, HookResponse::Abort { reason } if reason.contains("security")),
        "Expected Abort, got: {response:?}"
    );
}

#[tokio::test]
async fn test_command_hook_override_result() {
    let response = execute_hook_script(
        r#"echo '{"action":"override_result","text":"custom output"}'"#,
        &make_payload(HookEvent::ToolCallAfter),
        Some(Duration::from_secs(5)),
    )
    .await;
    assert!(
        matches!(&response, HookResponse::OverrideResult { text } if text == "custom output"),
        "Expected OverrideResult, got: {response:?}"
    );
}

// ── Payload Forwarding ──────────────────────────────────────────

#[tokio::test]
async fn test_command_hook_receives_full_payload() {
    // Script reads stdin, checks for expected fields, returns pass/abort based on content.
    let script = r#"
        input=$(cat)
        echo "$input" | grep -q "test-session-123" && \
        echo "$input" | grep -q "tool_call_before" && \
        echo "$input" | grep -q "bash" && \
        echo '{"action":"pass"}' || \
        echo '{"action":"abort","reason":"payload missing fields"}'
    "#;
    let response = execute_hook_script(
        script,
        &make_payload(HookEvent::ToolCallBefore),
        Some(Duration::from_secs(5)),
    )
    .await;
    assert!(
        matches!(response, HookResponse::Pass),
        "Hook should receive full payload with session_id, event, and data. Got: {response:?}"
    );
}

// ── Error Handling ──────────────────────────────────────────────

#[tokio::test]
async fn test_hook_timeout_returns_pass() {
    let response = execute_hook_script(
        "sleep 30",
        &make_payload(HookEvent::SessionStart),
        Some(Duration::from_millis(100)),
    )
    .await;
    assert!(
        matches!(response, HookResponse::Pass),
        "Timeout should fail-open to Pass"
    );
}

#[tokio::test]
async fn test_hook_nonzero_exit_returns_pass() {
    let response = execute_hook_script(
        "exit 1",
        &make_payload(HookEvent::SessionStart),
        Some(Duration::from_secs(5)),
    )
    .await;
    assert!(
        matches!(response, HookResponse::Pass),
        "Non-zero exit should fail-open to Pass"
    );
}

#[tokio::test]
async fn test_hook_invalid_json_returns_pass() {
    let response = execute_hook_script(
        "echo 'not valid json at all'",
        &make_payload(HookEvent::SessionStart),
        Some(Duration::from_secs(5)),
    )
    .await;
    assert!(
        matches!(response, HookResponse::Pass),
        "Invalid JSON should fail-open to Pass"
    );
}

#[tokio::test]
async fn test_hook_empty_output_returns_pass() {
    let response = execute_hook_script(
        "cat > /dev/null",
        &make_payload(HookEvent::SessionStart),
        Some(Duration::from_secs(5)),
    )
    .await;
    assert!(
        matches!(response, HookResponse::Pass),
        "Empty output should return Pass"
    );
}

#[tokio::test]
async fn test_hook_invalid_command_returns_pass() {
    let response = execute_hook_script(
        "/nonexistent/binary/path",
        &make_payload(HookEvent::SessionStart),
        Some(Duration::from_secs(2)),
    )
    .await;
    assert!(
        matches!(response, HookResponse::Pass),
        "Invalid command should fail-open to Pass"
    );
}

// ── execute_hook dispatch ───────────────────────────────────────

#[tokio::test]
async fn test_execute_hook_dispatches_command_type() {
    let hook_def = make_hook_def(r#"echo '{"action":"pass"}'"#);
    let response = execute_hook(&hook_def, &make_payload(HookEvent::SessionStart)).await;
    assert!(matches!(response, HookResponse::Pass));
}

#[tokio::test]
async fn test_execute_hook_agent_type_stubs_pass() {
    let hook_def = HookDef {
        hook_type: HookType::Agent,
        command: String::new(),
        timeout_secs: 5,
        enabled: true,
        agent: None,
        http: None,
        instructions: Some("Check for PII".to_string()),
        model: None,
        url: None,
        authorization: None,
    };
    // Agent hook executor is a stub that returns Pass.
    let response = execute_hook(&hook_def, &make_payload(HookEvent::ToolCallBefore)).await;
    assert!(matches!(response, HookResponse::Pass));
}

// ── HookPayload Serialization ───────────────────────────────────

#[test]
fn test_payload_serialization_has_all_fields() {
    let payload = make_payload(HookEvent::ToolCallBefore);
    let json = serde_json::to_value(&payload).unwrap();

    assert!(json.get("event").is_some(), "Should have 'event' field");
    assert!(json.get("data").is_some(), "Should have 'data' field");
    assert!(json.get("session_id").is_some(), "Should have 'session_id' field");
    assert!(json.get("model").is_some(), "Should have 'model' field");
    assert_eq!(json["event"], "tool_call_before");
    assert_eq!(json["session_id"], "test-session-123");
}

#[test]
fn test_hook_response_roundtrip() {
    let cases = vec![
        (r#"{"action":"pass"}"#, "Pass"),
        (r#"{"action":"modify_prompt","text":"extra"}"#, "ModifyPrompt"),
        (r#"{"action":"abort","reason":"no"}"#, "Abort"),
        (r#"{"action":"override_result","text":"replaced"}"#, "OverrideResult"),
    ];
    for (json, expected_variant) in cases {
        let parsed: HookResponse = serde_json::from_str(json).unwrap();
        let variant_name = format!("{parsed:?}");
        assert!(
            variant_name.contains(expected_variant),
            "Expected {expected_variant} from {json}, got: {variant_name}"
        );
    }
}

// ── HooksConfig ─────────────────────────────────────────────────

#[test]
fn test_hooks_config_default_is_empty() {
    let config = HooksConfig::default();
    assert!(
        config.get(HookEvent::SessionStart).is_none(),
        "Default config should have no hooks"
    );
}

#[test]
fn test_all_hook_events_have_str_names() {
    // Verify all 29 events have valid string representations.
    assert_eq!(HookEvent::ALL.len(), 29);
    for event in HookEvent::ALL {
        let name = event.as_str();
        assert!(!name.is_empty(), "Event {event:?} should have non-empty name");
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "Event name should be snake_case: {name}"
        );
    }
}
