//! Integration tests for the permission pipeline.
//!
//! These tests do NOT require API credentials.
//! Run with: `cargo test -p oxicode-permissions --test live_permission_pipeline`

use oxicode_permissions::pipeline::{PermissionMode, PermissionPipeline, ToolPermissionLevel};
use oxicode_permissions::rules::PermissionRule;
use oxicode_permissions::PermissionDecision;

#[test]
fn test_bypass_mode_allows_all() {
    let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);

    let tools = [
        ("file_read", ToolPermissionLevel::ReadOnly),
        ("file_write", ToolPermissionLevel::FileWrite),
        ("bash", ToolPermissionLevel::ShellExec),
        ("some_system_tool", ToolPermissionLevel::System),
    ];

    for (name, level) in &tools {
        let decision = pipeline.check(name, *level, &serde_json::json!({}));
        assert!(
            matches!(decision, PermissionDecision::Allow),
            "Bypass mode should allow {name}, got: {decision:?}"
        );
    }
}

#[test]
fn test_default_mode_allows_readonly() {
    let pipeline = PermissionPipeline::new(PermissionMode::Default, vec![]);

    let decision = pipeline.check(
        "file_read",
        ToolPermissionLevel::ReadOnly,
        &serde_json::json!({}),
    );
    assert!(
        matches!(decision, PermissionDecision::Allow),
        "Default mode should auto-allow ReadOnly tools, got: {decision:?}"
    );
}

#[test]
fn test_default_mode_asks_for_shell() {
    let pipeline = PermissionPipeline::new(PermissionMode::Default, vec![]);

    let decision = pipeline.check(
        "bash",
        ToolPermissionLevel::ShellExec,
        &serde_json::json!({"command": "echo test"}),
    );
    assert!(
        matches!(decision, PermissionDecision::Ask(_)),
        "Default mode should Ask for ShellExec tools, got: {decision:?}"
    );
}

#[test]
fn test_default_mode_asks_for_file_write() {
    let pipeline = PermissionPipeline::new(PermissionMode::Default, vec![]);

    let decision = pipeline.check(
        "file_write",
        ToolPermissionLevel::FileWrite,
        &serde_json::json!({"file_path": "/tmp/test.txt"}),
    );
    assert!(
        matches!(decision, PermissionDecision::Ask(_)),
        "Default mode should Ask for FileWrite tools, got: {decision:?}"
    );
}

#[test]
fn test_approval_only_mode_asks_for_writes() {
    let pipeline = PermissionPipeline::new(PermissionMode::ApprovalOnly, vec![]);

    let decision = pipeline.check(
        "file_write",
        ToolPermissionLevel::FileWrite,
        &serde_json::json!({"file_path": "/tmp/test.txt"}),
    );
    assert!(
        matches!(decision, PermissionDecision::Ask(_)),
        "ApprovalOnly mode should Ask for FileWrite, got: {decision:?}"
    );
}

#[test]
fn test_approval_only_mode_asks_for_shell() {
    let pipeline = PermissionPipeline::new(PermissionMode::ApprovalOnly, vec![]);

    let decision = pipeline.check(
        "bash",
        ToolPermissionLevel::ShellExec,
        &serde_json::json!({"command": "echo hi"}),
    );
    assert!(
        matches!(decision, PermissionDecision::Ask(_)),
        "ApprovalOnly mode should Ask for ShellExec, got: {decision:?}"
    );
}

#[test]
fn test_deny_rule_blocks_tool() {
    // Deny rules are evaluated in Layer 5, which runs in Default mode.
    let deny_rules = vec![PermissionRule::deny("bash", Some("echo test"))];
    let pipeline = PermissionPipeline::new(PermissionMode::Default, deny_rules);

    let decision = pipeline.check(
        "bash",
        ToolPermissionLevel::ShellExec,
        &serde_json::json!({"command": "echo test"}),
    );
    assert!(
        matches!(decision, PermissionDecision::Deny(_)),
        "Deny rule should block bash in Default mode, got: {decision:?}"
    );
}

#[test]
fn test_allow_rule_auto_approves() {
    let allow_rules = vec![PermissionRule::allow("bash", Some("echo.*"))];
    let pipeline = PermissionPipeline::new(PermissionMode::Default, allow_rules);

    let decision = pipeline.check(
        "bash",
        ToolPermissionLevel::ShellExec,
        &serde_json::json!({"command": "echo hello"}),
    );
    assert!(
        matches!(decision, PermissionDecision::Allow),
        "Allow rule should auto-approve bash in Default mode, got: {decision:?}"
    );
}

#[test]
fn test_denial_recording() {
    let pipeline = PermissionPipeline::new(PermissionMode::Default, vec![]);

    pipeline.record_denial("bash", "User denied shell access");
    let denials = pipeline.denial_history();

    assert!(
        !denials.is_empty(),
        "Should have recorded at least one denial"
    );
    // denial_history returns Vec<(tool_name, reason, timestamp)>
    assert!(
        denials.iter().any(|(name, _, _)| name == "bash"),
        "Denial should be for 'bash'"
    );
}

#[test]
fn test_permission_mode_parse() {
    assert_eq!(PermissionMode::parse("default"), PermissionMode::Default);
    assert_eq!(PermissionMode::parse("bypass"), PermissionMode::Bypass);
    assert_eq!(
        PermissionMode::parse("accept_edits"),
        PermissionMode::ApprovalOnly
    );
    assert_eq!(
        PermissionMode::parse("approval_only"),
        PermissionMode::ApprovalOnly
    );
    // Unknown strings should default to Default.
    assert_eq!(PermissionMode::parse("unknown"), PermissionMode::Default);
}
