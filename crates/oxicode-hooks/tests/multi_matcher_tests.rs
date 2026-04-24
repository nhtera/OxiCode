//! End-to-end tests for multi-matcher hook routing.
//!
//! Covers the Phase 1 bug-fix scenarios from
//! `plans/260425-0034-tui-parity-remaining-gaps/phase-01-multi-hook-matcher-routing.md`:
//!
//! 1. Two matcher groups on the same event fire the right hook per tool.
//! 2. Three groups with `.*` last: catch-all fires for uncaught tools,
//!    specific groups fire for matching tools (plus the catch-all).
//! 3. Back-compat: old flat TOML config (no matcher) still fires for all tools.
//! 4. Multiple hooks per group all fire in declaration order.
//!
//! Each hook script appends a distinct token to a per-test log file;
//! tests assert the expected tokens appear in order.

use std::path::PathBuf;

use oxicode_hooks::config::HooksConfig;
use oxicode_hooks::events::{HookEvent, HookResponse};
use oxicode_hooks::manager::{HookFields, HookManager};

/// Per-test log file derived from the test name so parallel tests don't collide.
fn log_path(test_name: &str) -> PathBuf {
    let dir = std::env::temp_dir();
    dir.join(format!("oxicode-multi-matcher-{test_name}.log"))
}

fn clear_log(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}

fn read_log(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Build a hook that appends `token` to `log_path` and exits.
fn echo_hook_command(token: &str, log_path: &std::path::Path) -> String {
    format!("echo {} >> {}", token, log_path.display())
}

#[tokio::test]
async fn two_matcher_groups_route_by_tool_name() {
    let log = log_path("two_groups");
    clear_log(&log);

    let json = serde_json::json!({
        "PreToolUse": [
            {
                "matcher": "Bash",
                "hooks": [{"type": "command", "command": echo_hook_command("BASH", &log)}]
            },
            {
                "matcher": "FileWrite",
                "hooks": [{"type": "command", "command": echo_hook_command("WRITE", &log)}]
            }
        ]
    });

    let config = HooksConfig::from_claude_json(&json);
    let mgr = HookManager::new(config);

    // Fire for Bash → only BASH should appear.
    let resp = mgr
        .fire_with_fields(
            HookEvent::PreToolUse,
            HookFields::for_tool("Bash", serde_json::json!({})),
        )
        .await;
    assert!(matches!(resp, HookResponse::Pass));

    // Fire for FileWrite → WRITE appended.
    let resp = mgr
        .fire_with_fields(
            HookEvent::PreToolUse,
            HookFields::for_tool("FileWrite", serde_json::json!({})),
        )
        .await;
    assert!(matches!(resp, HookResponse::Pass));

    let contents = read_log(&log);
    assert!(
        contents.contains("BASH"),
        "BASH token missing: {contents:?}"
    );
    assert!(
        contents.contains("WRITE"),
        "WRITE token missing: {contents:?}"
    );
    // And they appear in the order they were fired.
    let bash_pos = contents.find("BASH").unwrap();
    let write_pos = contents.find("WRITE").unwrap();
    assert!(bash_pos < write_pos, "execution order preserved");

    clear_log(&log);
}

#[tokio::test]
async fn catch_all_matcher_runs_after_specific_ones() {
    let log = log_path("catch_all");
    clear_log(&log);

    let json = serde_json::json!({
        "PreToolUse": [
            {
                "matcher": "Bash",
                "hooks": [{"type": "command", "command": echo_hook_command("BASH", &log)}]
            },
            {
                "matcher": "FileWrite",
                "hooks": [{"type": "command", "command": echo_hook_command("WRITE", &log)}]
            },
            {
                "matcher": ".*",
                "hooks": [{"type": "command", "command": echo_hook_command("ANY", &log)}]
            }
        ]
    });
    let config = HooksConfig::from_claude_json(&json);
    let mgr = HookManager::new(config);

    // Bash tool: both BASH (specific) and ANY (catch-all) fire.
    mgr.fire_with_fields(
        HookEvent::PreToolUse,
        HookFields::for_tool("Bash", serde_json::json!({})),
    )
    .await;

    let contents = read_log(&log);
    assert!(contents.contains("BASH"));
    assert!(contents.contains("ANY"));
    // Declaration order: BASH (line 1) before ANY (line 2).
    let bash_pos = contents.find("BASH").unwrap();
    let any_pos = contents.find("ANY").unwrap();
    assert!(
        bash_pos < any_pos,
        "specific matcher runs before catch-all: {contents:?}"
    );

    clear_log(&log);

    // Read tool: only ANY fires (no specific matcher for Read).
    mgr.fire_with_fields(
        HookEvent::PreToolUse,
        HookFields::for_tool("Read", serde_json::json!({})),
    )
    .await;
    let contents = read_log(&log);
    assert!(contents.contains("ANY"));
    assert!(!contents.contains("BASH"));
    assert!(!contents.contains("WRITE"));

    clear_log(&log);
}

#[tokio::test]
async fn legacy_flat_toml_config_still_fires() {
    let log = log_path("legacy_flat");
    clear_log(&log);

    // Old-style TOML: single command string per event, no matcher.
    let toml_str = format!("PreToolUse = {:?}", echo_hook_command("LEGACY", &log));
    let value: toml::Value = toml_str.parse().unwrap();
    let config = HooksConfig::from_toml_value(&value);
    let mgr = HookManager::new(config);

    // Should fire for any tool name (no matcher == always match).
    for tool in &["Bash", "FileWrite", "Read", "Edit"] {
        mgr.fire_with_fields(
            HookEvent::PreToolUse,
            HookFields::for_tool(*tool, serde_json::json!({})),
        )
        .await;
    }

    let contents = read_log(&log);
    let occurrences = contents.matches("LEGACY").count();
    assert_eq!(
        occurrences, 4,
        "legacy no-matcher hook should fire once per tool call; got: {contents:?}"
    );

    clear_log(&log);
}

#[tokio::test]
async fn multiple_hooks_in_one_group_all_fire_in_order() {
    let log = log_path("multi_in_group");
    clear_log(&log);

    let json = serde_json::json!({
        "PreToolUse": [
            {
                "matcher": ".*",
                "hooks": [
                    {"type": "command", "command": echo_hook_command("FIRST", &log)},
                    {"type": "command", "command": echo_hook_command("SECOND", &log)},
                    {"type": "command", "command": echo_hook_command("THIRD", &log)}
                ]
            }
        ]
    });
    let config = HooksConfig::from_claude_json(&json);
    let mgr = HookManager::new(config);

    mgr.fire_with_fields(
        HookEvent::PreToolUse,
        HookFields::for_tool("Bash", serde_json::json!({})),
    )
    .await;

    let contents = read_log(&log);
    let first = contents.find("FIRST").expect("FIRST missing");
    let second = contents.find("SECOND").expect("SECOND missing");
    let third = contents.find("THIRD").expect("THIRD missing");
    assert!(
        first < second && second < third,
        "execution order: {contents:?}"
    );

    clear_log(&log);
}

#[tokio::test]
async fn abort_from_any_hook_short_circuits_the_group() {
    let log = log_path("abort_short_circuit");
    clear_log(&log);

    // Second hook aborts; third hook should NOT run.
    let json = serde_json::json!({
        "PreToolUse": [
            {
                "matcher": ".*",
                "hooks": [
                    {"type": "command", "command": echo_hook_command("BEFORE", &log)},
                    {"type": "command", "command": r#"echo '{"action":"abort","reason":"policy"}'"#},
                    {"type": "command", "command": echo_hook_command("AFTER", &log)}
                ]
            }
        ]
    });
    let config = HooksConfig::from_claude_json(&json);
    let mgr = HookManager::new(config);

    let resp = mgr
        .fire_with_fields(
            HookEvent::PreToolUse,
            HookFields::for_tool("Bash", serde_json::json!({})),
        )
        .await;
    assert!(
        matches!(resp, HookResponse::Abort { ref reason } if reason == "policy"),
        "abort should short-circuit and propagate, got: {resp:?}"
    );

    let contents = read_log(&log);
    assert!(contents.contains("BEFORE"));
    assert!(
        !contents.contains("AFTER"),
        "post-abort hooks must not run: {contents:?}"
    );

    clear_log(&log);
}

#[tokio::test]
async fn has_hook_true_when_any_matcher_group_present() {
    let json = serde_json::json!({
        "PreToolUse": [
            {"matcher": "Bash", "hooks": [{"type":"command","command":"true"}]}
        ]
    });
    let config = HooksConfig::from_claude_json(&json);
    let mgr = HookManager::new(config);

    // has_hook is tool-agnostic → true even if tool name wouldn't match the regex.
    assert!(mgr.has_hook(HookEvent::PreToolUse));
    assert!(!mgr.has_hook(HookEvent::SessionStart));
}

#[tokio::test]
async fn payload_field_source_in_json_data_routes_matcher() {
    // The higher-level `fire()` entry point reads `tool` or `tool_name` from
    // the JSON data blob; make sure multi-matcher routing works through it.
    let log = log_path("fire_blob");
    clear_log(&log);

    let json = serde_json::json!({
        "PreToolUse": [
            {
                "matcher": "Bash",
                "hooks": [{"type": "command", "command": echo_hook_command("BASH_ONLY", &log)}]
            }
        ]
    });
    let config = HooksConfig::from_claude_json(&json);
    let mgr = HookManager::new(config);

    // Via the JSON-data compatibility entry point.
    mgr.fire(
        HookEvent::PreToolUse,
        serde_json::json!({"tool_name": "Bash"}),
    )
    .await;

    let contents = read_log(&log);
    assert!(contents.contains("BASH_ONLY"), "got: {contents:?}");

    clear_log(&log);

    // Now fire for Read — no matcher group accepts it, nothing should be written.
    mgr.fire(
        HookEvent::PreToolUse,
        serde_json::json!({"tool_name": "Read"}),
    )
    .await;
    let contents = read_log(&log);
    assert!(
        contents.is_empty(),
        "Read should not trigger Bash-matcher hook: {contents:?}"
    );

    clear_log(&log);
}
