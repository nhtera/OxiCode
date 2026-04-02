//! Integration tests for the 6-layer permission pipeline.

use oxicode_permissions::pipeline::{
    PermissionDecision, PermissionMode, PermissionPipeline, ToolPermissionLevel,
};
use oxicode_permissions::rules::PermissionRule;

#[test]
fn test_readonly_tools_always_pass() {
    let pipeline = PermissionPipeline::new(PermissionMode::Default, vec![]);
    let tools = ["file_read", "glob", "grep", "list_files"];

    for tool in &tools {
        let decision = pipeline.check(tool, ToolPermissionLevel::ReadOnly, &serde_json::json!({}));
        assert_eq!(
            decision,
            PermissionDecision::Allow,
            "ReadOnly tool {tool} should always be allowed"
        );
    }
}

#[test]
fn test_bypass_mode_allows_everything() {
    let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
    let cases = [
        ("bash", ToolPermissionLevel::ShellExec),
        ("file_write", ToolPermissionLevel::FileWrite),
        ("system_exec", ToolPermissionLevel::System),
    ];

    for (tool, level) in &cases {
        let decision = pipeline.check(tool, *level, &serde_json::json!({}));
        assert_eq!(
            decision,
            PermissionDecision::Allow,
            "Bypass mode should allow {tool}"
        );
    }
}

#[test]
fn test_approval_mode_asks_for_non_readonly() {
    let pipeline = PermissionPipeline::new(PermissionMode::ApprovalOnly, vec![]);

    let decision = pipeline.check(
        "file_write",
        ToolPermissionLevel::FileWrite,
        &serde_json::json!({}),
    );
    assert!(matches!(decision, PermissionDecision::Ask(_)));

    let decision = pipeline.check(
        "file_read",
        ToolPermissionLevel::ReadOnly,
        &serde_json::json!({}),
    );
    assert_eq!(decision, PermissionDecision::Allow);
}

#[test]
fn test_dangerous_commands_caught() {
    let pipeline = PermissionPipeline::new(PermissionMode::Default, vec![]);
    let dangerous = [
        r#"{"command": "rm -rf /"}"#,
        r#"{"command": "rm -rf ~"}"#,
    ];

    for cmd_json in &dangerous {
        let input: serde_json::Value = serde_json::from_str(cmd_json).unwrap();
        let decision = pipeline.check("bash", ToolPermissionLevel::ShellExec, &input);
        assert!(
            matches!(
                decision,
                PermissionDecision::Ask(_) | PermissionDecision::Deny(_)
            ),
            "Dangerous command should not be auto-allowed: {cmd_json}"
        );
    }
}

#[test]
fn test_allow_rule_applied() {
    let rules = vec![PermissionRule::allow("bash", Some("echo.*"))];
    let pipeline = PermissionPipeline::new(PermissionMode::Default, rules);

    let decision = pipeline.check(
        "bash",
        ToolPermissionLevel::ShellExec,
        &serde_json::json!({"command": "echo hello world"}),
    );
    assert_eq!(decision, PermissionDecision::Allow);
}

#[test]
fn test_deny_rule_applied() {
    let rules = vec![PermissionRule::deny("bash", Some("deploy"))];
    let pipeline = PermissionPipeline::new(PermissionMode::Default, rules);

    let decision = pipeline.check(
        "bash",
        ToolPermissionLevel::ShellExec,
        &serde_json::json!({"command": "deploy production"}),
    );
    assert!(matches!(decision, PermissionDecision::Deny(_)));
}

#[test]
fn test_dangerous_overrides_allow_rule() {
    let rules = vec![PermissionRule::allow("bash", None)];
    let pipeline = PermissionPipeline::new(PermissionMode::Default, rules);

    let decision = pipeline.check(
        "bash",
        ToolPermissionLevel::ShellExec,
        &serde_json::json!({"command": "rm -rf /"}),
    );
    assert!(
        matches!(decision, PermissionDecision::Ask(_)),
        "Dangerous command should override allow rule"
    );
}

#[test]
fn test_denial_tracking() {
    let pipeline = PermissionPipeline::new(PermissionMode::Default, vec![]);
    pipeline.record_denial("bash", "user denied");
    pipeline.record_denial("file_write", "dangerous path");

    let history = pipeline.denial_history();
    assert_eq!(history.len(), 2);
}

#[test]
fn test_permission_mode_parsing() {
    assert_eq!(PermissionMode::parse("bypass"), PermissionMode::Bypass);
    assert_eq!(
        PermissionMode::parse("approval_only"),
        PermissionMode::ApprovalOnly
    );
    assert_eq!(
        PermissionMode::parse("accept_edits"),
        PermissionMode::ApprovalOnly
    );
    assert_eq!(PermissionMode::parse("default"), PermissionMode::Default);
    assert_eq!(PermissionMode::parse("unknown"), PermissionMode::Default);
}
