//! Tests for hook prompt modification — PreQuery modify, PostQuery override, composition, error handling.
//!
//! No API key needed — uses shell scripts via execute_hook_script.
//! Run with: `cargo test -p oxicode-hooks --test hook_prompt_modification_tests`

use std::time::Duration;

use oxicode_hooks::events::{HookEvent, HookPayload, HookResponse};
use oxicode_hooks::executor::execute_hook_script;

fn test_payload(event: HookEvent) -> HookPayload {
    let mut p = HookPayload::new(event, "test-session");
    p.model = Some("test-model".to_string());
    p
}

#[tokio::test]
async fn test_pre_query_hook_modifies_prompt() {
    let payload = test_payload(HookEvent::UserPromptSubmit);

    let response = execute_hook_script(
        r#"echo '{"action":"modify_prompt","text":"Always respond in French"}'"#,
        &payload,
        Some(Duration::from_secs(5)),
    )
    .await;

    match response {
        HookResponse::ModifyPrompt { text } => {
            assert_eq!(text, "Always respond in French");
        }
        other => panic!("expected ModifyPrompt, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_post_query_hook_overrides_result() {
    let payload = test_payload(HookEvent::Stop);

    let response = execute_hook_script(
        r#"echo '{"action":"override_result","text":"replaced output"}'"#,
        &payload,
        Some(Duration::from_secs(5)),
    )
    .await;

    match response {
        HookResponse::OverrideResult { text } => {
            assert_eq!(text, "replaced output");
        }
        other => panic!("expected OverrideResult, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_multiple_hooks_compose() {
    let payload = test_payload(HookEvent::UserPromptSubmit);

    // Simulate two hooks by calling execute_hook_script twice
    // and composing the results.
    let resp_a = execute_hook_script(
        r#"echo '{"action":"modify_prompt","text":"rule A"}'"#,
        &payload,
        Some(Duration::from_secs(5)),
    )
    .await;

    let resp_b = execute_hook_script(
        r#"echo '{"action":"modify_prompt","text":"rule B"}'"#,
        &payload,
        Some(Duration::from_secs(5)),
    )
    .await;

    // Collect modifications in order.
    let mut modifications = Vec::new();
    if let HookResponse::ModifyPrompt { text } = resp_a {
        modifications.push(text);
    }
    if let HookResponse::ModifyPrompt { text } = resp_b {
        modifications.push(text);
    }

    assert_eq!(
        modifications.len(),
        2,
        "both hooks should produce modifications"
    );
    assert_eq!(modifications[0], "rule A");
    assert_eq!(modifications[1], "rule B");
}

#[tokio::test]
async fn test_hook_error_does_not_block_execution() {
    let payload = test_payload(HookEvent::UserPromptSubmit);

    // Hook script exits with error code — should return Pass (fail-open).
    let response = execute_hook_script("exit 1", &payload, Some(Duration::from_secs(5))).await;

    assert!(
        matches!(response, HookResponse::Pass),
        "errored hook should return Pass (fail-open), got: {response:?}"
    );
}
