//! Integration tests for TaskManager lifecycle, multi-task coordination, and cleanup.
//!
//! No API key needed — pure task management logic.
//! Run with: `cargo test -p oxicode-tasks --test task_lifecycle_integration_tests`

use oxicode_tasks::{TaskManager, TaskStatus, TaskType};

fn make_manager() -> TaskManager {
    TaskManager::default()
}

fn bash_task(mgr: &mut TaskManager, cmd: &str) -> String {
    mgr.create_task(TaskType::LocalBash {
        command: cmd.into(),
    })
}

fn agent_task(mgr: &mut TaskManager, prompt: &str) -> String {
    mgr.create_task(TaskType::LocalAgent {
        prompt: prompt.into(),
        model: "test-model".into(),
    })
}

// ═══════════════════════════════════════════════════════════════════
// A. Task Lifecycle — Full State Machine
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_full_lifecycle_pending_to_completed() {
    let mut mgr = make_manager();
    let id = bash_task(&mut mgr, "echo hi");

    // Pending → Running → Completed.
    assert!(matches!(mgr.get_task(&id).unwrap().status, TaskStatus::Pending));
    assert!(mgr.get_task(&id).unwrap().completed_at.is_none());

    mgr.update_status(&id, TaskStatus::Running);
    assert!(matches!(mgr.get_task(&id).unwrap().status, TaskStatus::Running));
    assert!(mgr.get_task(&id).unwrap().completed_at.is_none());

    mgr.update_status(&id, TaskStatus::Completed { exit_code: 0 });
    assert!(matches!(mgr.get_task(&id).unwrap().status, TaskStatus::Completed { exit_code: 0 }));
    assert!(mgr.get_task(&id).unwrap().completed_at.is_some());
}

#[test]
fn test_full_lifecycle_pending_to_failed() {
    let mut mgr = make_manager();
    let id = bash_task(&mut mgr, "false");

    mgr.update_status(&id, TaskStatus::Running);
    mgr.update_status(
        &id,
        TaskStatus::Failed {
            error: "exit code 1".into(),
        },
    );

    let task = mgr.get_task(&id).unwrap();
    assert!(matches!(task.status, TaskStatus::Failed { .. }));
    assert!(task.completed_at.is_some());
}

#[test]
fn test_full_lifecycle_pending_to_killed() {
    let mut mgr = make_manager();
    let id = bash_task(&mut mgr, "sleep 999");

    mgr.update_status(&id, TaskStatus::Running);
    mgr.kill_task(&id).unwrap();

    let task = mgr.get_task(&id).unwrap();
    assert!(matches!(task.status, TaskStatus::Killed));
    assert!(task.completed_at.is_some());
}

// ═══════════════════════════════════════════════════════════════════
// B. Multi-Task Coordination
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_multiple_tasks_independent_status() {
    let mut mgr = make_manager();
    let id1 = bash_task(&mut mgr, "echo 1");
    let id2 = bash_task(&mut mgr, "echo 2");
    let id3 = bash_task(&mut mgr, "echo 3");

    // Each task has independent status.
    mgr.update_status(&id1, TaskStatus::Running);
    mgr.update_status(&id2, TaskStatus::Completed { exit_code: 0 });
    mgr.update_status(
        &id3,
        TaskStatus::Failed {
            error: "boom".into(),
        },
    );

    assert!(matches!(mgr.get_task(&id1).unwrap().status, TaskStatus::Running));
    assert!(matches!(mgr.get_task(&id2).unwrap().status, TaskStatus::Completed { .. }));
    assert!(matches!(mgr.get_task(&id3).unwrap().status, TaskStatus::Failed { .. }));
}

#[test]
fn test_list_tasks_returns_all() {
    let mut mgr = make_manager();
    let _id1 = bash_task(&mut mgr, "task 1");
    let _id2 = agent_task(&mut mgr, "task 2");
    let _id3 = bash_task(&mut mgr, "task 3");

    let all = mgr.list_tasks();
    assert_eq!(all.len(), 3);
}

#[test]
fn test_remove_task() {
    let mut mgr = make_manager();
    let id = bash_task(&mut mgr, "removable");
    assert!(mgr.get_task(&id).is_some());

    mgr.remove_task(&id);
    assert!(mgr.get_task(&id).is_none());
}

// ═══════════════════════════════════════════════════════════════════
// C. Kill Validation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_kill_pending_task_fails() {
    let mut mgr = make_manager();
    let id = bash_task(&mut mgr, "pending");

    let result = mgr.kill_task(&id);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not running"));
}

#[test]
fn test_kill_completed_task_fails() {
    let mut mgr = make_manager();
    let id = bash_task(&mut mgr, "done");
    mgr.update_status(&id, TaskStatus::Running);
    mgr.update_status(&id, TaskStatus::Completed { exit_code: 0 });

    let result = mgr.kill_task(&id);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not running"));
}

#[test]
fn test_kill_nonexistent_task_fails() {
    let mut mgr = make_manager();
    let result = mgr.kill_task("nonexistent-id");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

#[test]
fn test_kill_already_killed_task_fails() {
    let mut mgr = make_manager();
    let id = bash_task(&mut mgr, "to-kill");
    mgr.update_status(&id, TaskStatus::Running);
    mgr.kill_task(&id).unwrap();

    // Second kill attempt should fail — status is now Killed, not Running.
    let result = mgr.kill_task(&id);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════
// D. Cleanup — Auto-Expiry
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_cleanup_retains_running_tasks() {
    let mut mgr = make_manager();
    let id = bash_task(&mut mgr, "long running");
    mgr.update_status(&id, TaskStatus::Running);

    mgr.cleanup_completed();
    assert!(
        mgr.get_task(&id).is_some(),
        "running tasks should NOT be cleaned up"
    );
}

#[test]
fn test_cleanup_retains_pending_tasks() {
    let mut mgr = make_manager();
    let id = bash_task(&mut mgr, "pending");

    mgr.cleanup_completed();
    assert!(
        mgr.get_task(&id).is_some(),
        "pending tasks should NOT be cleaned up"
    );
}

#[test]
fn test_cleanup_retains_recent_completed() {
    let mut mgr = make_manager();
    let id = bash_task(&mut mgr, "just completed");
    mgr.update_status(&id, TaskStatus::Running);
    mgr.update_status(&id, TaskStatus::Completed { exit_code: 0 });

    mgr.cleanup_completed();
    assert!(
        mgr.get_task(&id).is_some(),
        "recently completed tasks should be retained"
    );
}

// ═══════════════════════════════════════════════════════════════════
// E. Task Type Variants
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_create_bash_task() {
    let mut mgr = make_manager();
    let id = mgr.create_task(TaskType::LocalBash {
        command: "cargo test".into(),
    });
    let task = mgr.get_task(&id).unwrap();
    assert!(matches!(task.task_type, TaskType::LocalBash { .. }));
}

#[test]
fn test_create_agent_task() {
    let mut mgr = make_manager();
    let id = mgr.create_task(TaskType::LocalAgent {
        prompt: "review code".into(),
        model: "claude-sonnet-4".into(),
    });
    let task = mgr.get_task(&id).unwrap();
    assert!(matches!(task.task_type, TaskType::LocalAgent { .. }));
}

#[test]
fn test_create_monitor_task() {
    let mut mgr = make_manager();
    let id = mgr.create_task(TaskType::Monitor {
        interval_secs: 30,
        command: "tail -f /var/log/app.log".into(),
    });
    let task = mgr.get_task(&id).unwrap();
    assert!(matches!(task.task_type, TaskType::Monitor { .. }));
}

#[test]
fn test_unique_task_ids() {
    let mut mgr = make_manager();
    let mut ids = Vec::new();
    for i in 0..20 {
        ids.push(bash_task(&mut mgr, &format!("task-{i}")));
    }

    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 20, "all task IDs should be unique");
}

// ═══════════════════════════════════════════════════════════════════
// F. Mixed Task Lifecycle Scenario
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_realistic_multi_task_scenario() {
    let mut mgr = make_manager();

    // Create 5 tasks simulating a real workflow.
    let build = bash_task(&mut mgr, "cargo build");
    let lint = bash_task(&mut mgr, "cargo clippy");
    let test = bash_task(&mut mgr, "cargo test");
    let review = agent_task(&mut mgr, "review code changes");
    let deploy = bash_task(&mut mgr, "deploy.sh");

    // Phase 1: Build + Lint run in parallel.
    mgr.update_status(&build, TaskStatus::Running);
    mgr.update_status(&lint, TaskStatus::Running);

    // Build completes, lint fails.
    mgr.update_status(&build, TaskStatus::Completed { exit_code: 0 });
    mgr.update_status(
        &lint,
        TaskStatus::Failed {
            error: "clippy warning".into(),
        },
    );

    // Phase 2: Test starts (depends on build succeeding).
    let build_ok = matches!(
        mgr.get_task(&build).unwrap().status,
        TaskStatus::Completed { exit_code: 0 }
    );
    assert!(build_ok);
    mgr.update_status(&test, TaskStatus::Running);
    mgr.update_status(&test, TaskStatus::Completed { exit_code: 0 });

    // Phase 3: Review agent runs.
    mgr.update_status(&review, TaskStatus::Running);
    mgr.update_status(&review, TaskStatus::Completed { exit_code: 0 });

    // Phase 4: Deploy blocked by lint failure.
    let lint_ok = matches!(
        mgr.get_task(&lint).unwrap().status,
        TaskStatus::Completed { .. }
    );
    assert!(!lint_ok, "lint failed — deploy should NOT proceed");

    // Deploy stays pending.
    assert!(matches!(
        mgr.get_task(&deploy).unwrap().status,
        TaskStatus::Pending
    ));

    // Summary: 3 completed, 1 failed, 1 pending.
    let all = mgr.list_tasks();
    let completed = all
        .iter()
        .filter(|t| matches!(t.status, TaskStatus::Completed { .. }))
        .count();
    let failed = all
        .iter()
        .filter(|t| matches!(t.status, TaskStatus::Failed { .. }))
        .count();
    let pending = all
        .iter()
        .filter(|t| matches!(t.status, TaskStatus::Pending))
        .count();

    assert_eq!(completed, 3);
    assert_eq!(failed, 1);
    assert_eq!(pending, 1);
}
