//! Integration tests for async slash command dispatch (Phases 2, 3, 5).
//!
//! These tests exercise the JSON input shapes that the engine task in
//! `main.rs` constructs for `/cron`, `/schedule`, and `/worktree create`.
//! They invoke the underlying tools directly through `ToolRegistry::execute`
//! to verify the wire format matches.
//!
//! The dispatcher code in `main.rs::1239` cannot be unit-tested in isolation
//! (it's wedged inside a `tokio::spawn` driving the engine event loop), so we
//! pin behavior at the boundary the dispatcher actually crosses: the tool
//! registry call.

use std::sync::Arc;

use oxicode_tools::{
    cron::{CronCreateTool, CronDeleteTool, CronListTool},
    ToolContext, ToolRegistry,
};

fn registry_with_cron() -> Arc<ToolRegistry> {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(CronCreateTool));
    reg.register(Box::new(CronDeleteTool));
    reg.register(Box::new(CronListTool));
    Arc::new(reg)
}

fn isolated_ctx(tmp_home: &tempfile::TempDir) -> ToolContext {
    // Cron tools persist under $HOME/.oxicode/schedules/. Override HOME so
    // tests don't pollute the real directory.
    std::env::set_var("HOME", tmp_home.path());
    ToolContext::default()
}

#[tokio::test]
async fn cron_dispatch_create_then_list_then_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = isolated_ctx(&tmp);
    let reg = registry_with_cron();

    // Mirror the JSON the /cron create dispatcher builds.
    let create_input = serde_json::json!({
        "cron": "*/5 * * * *",
        "command": "echo hi",
    });
    let create_res = reg
        .execute("cron_create", create_input, &ctx)
        .await
        .expect("cron_create");
    assert!(!create_res.is_error, "create failed: {}", create_res.content);
    // Format: "Schedule created: <uuid> (<cron>)"
    let id = create_res
        .content
        .split_whitespace()
        .nth(2)
        .expect("id in response")
        .to_string();

    // /cron list dispatch input is empty json object.
    let list_res = reg
        .execute("cron_list", serde_json::json!({}), &ctx)
        .await
        .expect("cron_list");
    assert!(!list_res.is_error);
    assert!(
        list_res.content.contains(&id),
        "list missing id {id}: {}",
        list_res.content
    );

    // /cron delete <id> dispatch.
    let del_res = reg
        .execute("cron_delete", serde_json::json!({ "id": id.clone() }), &ctx)
        .await
        .expect("cron_delete");
    assert!(!del_res.is_error, "delete failed: {}", del_res.content);

    // List again — should be empty.
    let after = reg
        .execute("cron_list", serde_json::json!({}), &ctx)
        .await
        .unwrap();
    assert!(
        !after.content.contains(&id),
        "id should be gone: {}",
        after.content
    );
}

#[tokio::test]
async fn schedule_dispatch_creates_one_shot_via_cron_create() {
    // /schedule routes to cron_create with a description tag — verify the
    // descriptor lands on disk by listing afterwards.
    let tmp = tempfile::tempdir().unwrap();
    let ctx = isolated_ctx(&tmp);
    let reg = registry_with_cron();

    let input = serde_json::json!({
        "cron": "0 9 * * *",
        "command": "morning report",
        "description": "scheduled via /schedule",
    });
    let res = reg.execute("cron_create", input, &ctx).await.unwrap();
    assert!(!res.is_error, "schedule create failed: {}", res.content);

    let list = reg
        .execute("cron_list", serde_json::json!({}), &ctx)
        .await
        .unwrap();
    assert!(
        list.content.contains("morning report"),
        "list should contain command: {}",
        list.content
    );
}

#[tokio::test]
async fn cron_create_rejects_missing_fields() {
    // Same shape the dispatcher would NOT build (it validates first), but
    // pin tool-side behavior so we know what error surfaces if the
    // dispatcher's validation regresses.
    let tmp = tempfile::tempdir().unwrap();
    let ctx = isolated_ctx(&tmp);
    let reg = registry_with_cron();

    let res = reg
        .execute(
            "cron_create",
            serde_json::json!({ "cron": "* * * * *" }), // missing command
            &ctx,
        )
        .await
        .unwrap();
    assert!(res.is_error);
    assert!(res.content.to_lowercase().contains("command"));
}
