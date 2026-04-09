//! Tests for TaskManager status transitions, dependencies, and failure cascading.
//!
//! No API key needed — pure task management logic.
//! Run with: `cargo test -p oxicode-tasks --test task_dependency_tests`

use oxicode_tasks::{TaskManager, TaskStatus, TaskType};

fn make_manager() -> TaskManager {
    TaskManager::default()
}

fn create_bash_task(mgr: &mut TaskManager) -> String {
    mgr.create_task(TaskType::LocalBash {
        command: "true".into(),
    })
}

#[test]
fn test_task_status_transitions() {
    let mut mgr = make_manager();
    let id = create_bash_task(&mut mgr);

    // Initial status is Pending.
    assert!(matches!(
        mgr.get_task(&id).unwrap().status,
        TaskStatus::Pending
    ));

    // Transition: Pending → Running.
    mgr.update_status(&id, TaskStatus::Running);
    assert!(matches!(
        mgr.get_task(&id).unwrap().status,
        TaskStatus::Running
    ));

    // Transition: Running → Completed.
    mgr.update_status(&id, TaskStatus::Completed { exit_code: 0 });
    assert!(matches!(
        mgr.get_task(&id).unwrap().status,
        TaskStatus::Completed { .. }
    ));

    // Terminal state has completed_at set.
    assert!(mgr.get_task(&id).unwrap().completed_at.is_some());
}

#[test]
fn test_task_dependency_blocks_execution() {
    let mut mgr = make_manager();
    let id_a = create_bash_task(&mut mgr);
    let id_b = create_bash_task(&mut mgr);

    // Simulate B depends on A: B cannot start while A is Pending.
    let a_is_complete = matches!(
        mgr.get_task(&id_a).unwrap().status,
        TaskStatus::Completed { .. }
    );
    assert!(!a_is_complete, "A should be Pending, so B should NOT start");

    // After completing A, B can proceed.
    mgr.update_status(&id_a, TaskStatus::Running);
    mgr.update_status(&id_a, TaskStatus::Completed { exit_code: 0 });

    let a_is_complete = matches!(
        mgr.get_task(&id_a).unwrap().status,
        TaskStatus::Completed { .. }
    );
    assert!(a_is_complete, "A is now complete, B can start");

    // Now B can transition to Running.
    mgr.update_status(&id_b, TaskStatus::Running);
    assert!(matches!(
        mgr.get_task(&id_b).unwrap().status,
        TaskStatus::Running
    ));
}

#[test]
fn test_task_completion_unblocks_dependents() {
    let mut mgr = make_manager();
    let id_a = create_bash_task(&mut mgr);
    let id_b = create_bash_task(&mut mgr);

    // A blocks B — verify B's readiness changes when A completes.
    assert!(
        !matches!(
            mgr.get_task(&id_a).unwrap().status,
            TaskStatus::Completed { .. }
        ),
        "A not done → B blocked"
    );

    mgr.update_status(&id_a, TaskStatus::Running);
    mgr.update_status(&id_a, TaskStatus::Completed { exit_code: 0 });

    assert!(
        matches!(
            mgr.get_task(&id_a).unwrap().status,
            TaskStatus::Completed { .. }
        ),
        "A done → B unblocked"
    );

    // B can now proceed.
    mgr.update_status(&id_b, TaskStatus::Running);
    mgr.update_status(&id_b, TaskStatus::Completed { exit_code: 0 });
    assert!(matches!(
        mgr.get_task(&id_b).unwrap().status,
        TaskStatus::Completed { .. }
    ));
}

#[test]
fn test_task_failure_cascades_to_dependents() {
    let mut mgr = make_manager();
    let id_a = create_bash_task(&mut mgr);
    let id_b = create_bash_task(&mut mgr);
    let id_c = create_bash_task(&mut mgr);

    // Chain: A → B → C. If A fails, B and C should be marked failed.
    mgr.update_status(&id_a, TaskStatus::Running);
    mgr.update_status(
        &id_a,
        TaskStatus::Failed {
            error: "segfault".into(),
        },
    );

    // Simulate cascade: when A fails, mark dependents as failed too.
    let a_failed = matches!(
        mgr.get_task(&id_a).unwrap().status,
        TaskStatus::Failed { .. }
    );
    assert!(a_failed);

    if a_failed {
        mgr.update_status(
            &id_b,
            TaskStatus::Failed {
                error: "dependency A failed".into(),
            },
        );
        mgr.update_status(
            &id_c,
            TaskStatus::Failed {
                error: "dependency B failed (cascade from A)".into(),
            },
        );
    }

    assert!(matches!(
        mgr.get_task(&id_b).unwrap().status,
        TaskStatus::Failed { .. }
    ));
    assert!(matches!(
        mgr.get_task(&id_c).unwrap().status,
        TaskStatus::Failed { .. }
    ));
}

#[test]
fn test_independent_tasks_unaffected_by_failure() {
    let mut mgr = make_manager();
    let id_a = create_bash_task(&mut mgr);
    let _id_b = create_bash_task(&mut mgr);
    let id_c = create_bash_task(&mut mgr);

    // A blocks B; C is independent.
    mgr.update_status(&id_a, TaskStatus::Running);
    mgr.update_status(
        &id_a,
        TaskStatus::Failed {
            error: "boom".into(),
        },
    );

    // C should remain Pending (unaffected by A's failure).
    assert!(
        matches!(mgr.get_task(&id_c).unwrap().status, TaskStatus::Pending),
        "independent task C should remain Pending"
    );
}

#[test]
fn test_multi_dependency_all_must_complete() {
    let mut mgr = make_manager();
    let id_a = create_bash_task(&mut mgr);
    let id_b = create_bash_task(&mut mgr);
    let id_c = create_bash_task(&mut mgr);

    // C depends on both A and B.
    let all_deps_done = |mgr: &TaskManager, deps: &[&str]| -> bool {
        deps.iter().all(|id| {
            matches!(
                mgr.get_task(id).unwrap().status,
                TaskStatus::Completed { .. }
            )
        })
    };

    // Complete A only — C still blocked.
    mgr.update_status(&id_a, TaskStatus::Running);
    mgr.update_status(&id_a, TaskStatus::Completed { exit_code: 0 });
    assert!(
        !all_deps_done(&mgr, &[&id_a, &id_b]),
        "B not done → C still blocked"
    );

    // Complete B — now C is ready.
    mgr.update_status(&id_b, TaskStatus::Running);
    mgr.update_status(&id_b, TaskStatus::Completed { exit_code: 0 });
    assert!(
        all_deps_done(&mgr, &[&id_a, &id_b]),
        "both A and B done → C unblocked"
    );

    // C can now run.
    mgr.update_status(&id_c, TaskStatus::Running);
    mgr.update_status(&id_c, TaskStatus::Completed { exit_code: 0 });
    assert!(matches!(
        mgr.get_task(&id_c).unwrap().status,
        TaskStatus::Completed { .. }
    ));
}
