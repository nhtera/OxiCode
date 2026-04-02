use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::manager::{TaskManager, TaskStatus, TaskType};

/// Notification payload for a completed or failed task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNotification {
    pub task_id: String,
    pub task_type: String,
    pub status: String,
    pub message: String,
}

/// Format a notification as an XML tag suitable for injection into a prompt.
pub fn format_notification(notification: &TaskNotification) -> String {
    format!(
        r#"<task-notification task_id="{}" status="{}">{}</task-notification>"#,
        notification.task_id, notification.status, notification.message
    )
}

/// Derive a human-readable label for a `TaskType`.
fn task_type_label(task_type: &TaskType) -> &'static str {
    match task_type {
        TaskType::LocalBash { .. } => "bash",
        TaskType::LocalAgent { .. } => "agent",
        TaskType::Monitor { .. } => "monitor",
    }
}

/// Derive status string and notification message from a terminal `TaskStatus`.
/// Returns `None` for non-terminal statuses.
fn terminal_notification(
    task_id: &str,
    task_type: &TaskType,
    status: &TaskStatus,
) -> Option<TaskNotification> {
    match status {
        TaskStatus::Completed { exit_code } => Some(TaskNotification {
            task_id: task_id.to_string(),
            task_type: task_type_label(task_type).to_string(),
            status: "completed".to_string(),
            message: format!("Task {task_id} completed with exit code {exit_code}"),
        }),
        TaskStatus::Failed { error } => Some(TaskNotification {
            task_id: task_id.to_string(),
            task_type: task_type_label(task_type).to_string(),
            status: "failed".to_string(),
            message: format!("Task {task_id} failed: {error}"),
        }),
        TaskStatus::Killed => Some(TaskNotification {
            task_id: task_id.to_string(),
            task_type: task_type_label(task_type).to_string(),
            status: "killed".to_string(),
            message: format!("Task {task_id} was killed"),
        }),
        TaskStatus::Pending | TaskStatus::Running => None,
    }
}

/// Stateless helper — scan the manager and collect notifications for all terminal tasks.
/// Callers who need de-duplication should use `NotificationCollector` instead.
pub fn collect_notifications(manager: &TaskManager) -> Vec<TaskNotification> {
    manager
        .list_tasks()
        .into_iter()
        .filter_map(|entry| {
            terminal_notification(&entry.id, &entry.task_type, &entry.status)
        })
        .collect()
}

/// Stateful collector that remembers which task IDs have already been emitted,
/// so repeated calls only return newly-transitioned tasks.
#[derive(Debug, Default)]
pub struct NotificationCollector {
    notified: HashSet<String>,
}

impl NotificationCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return notifications for tasks that have newly reached a terminal state.
    pub fn check(&mut self, manager: &TaskManager) -> Vec<TaskNotification> {
        let mut new_notifications = Vec::new();

        for entry in manager.list_tasks() {
            if self.notified.contains(&entry.id) {
                continue;
            }
            if let Some(notif) =
                terminal_notification(&entry.id, &entry.task_type, &entry.status)
            {
                tracing::info!(
                    "notification task={} status={}",
                    entry.id,
                    notif.status
                );
                self.notified.insert(entry.id.clone());
                new_notifications.push(notif);
            }
        }

        new_notifications
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager::{new_with_dir, TaskManager, TaskStatus, TaskType};
    use uuid::Uuid;

    fn make_manager_with_task(status: TaskStatus) -> TaskManager {
        let tmp = std::env::temp_dir().join(format!("oxi-notif-{}", Uuid::new_v4()));
        let mut mgr = new_with_dir(tmp);
        let id = mgr.create_task(TaskType::LocalBash {
            command: "echo test".into(),
        });
        mgr.update_status(&id, status);
        mgr
    }

    #[test]
    fn format_notification_produces_xml() {
        let n = TaskNotification {
            task_id: "abc".into(),
            task_type: "bash".into(),
            status: "completed".into(),
            message: "done".into(),
        };
        let xml = format_notification(&n);
        assert!(xml.starts_with("<task-notification"));
        assert!(xml.contains(r#"task_id="abc""#));
        assert!(xml.contains("done"));
        assert!(xml.ends_with("</task-notification>"));
    }

    #[test]
    fn collector_deduplicates_notifications() {
        let mgr = make_manager_with_task(TaskStatus::Completed { exit_code: 0 });
        let mut collector = NotificationCollector::new();

        let first = collector.check(&mgr);
        assert_eq!(first.len(), 1);

        // Second call should produce nothing new.
        let second = collector.check(&mgr);
        assert!(second.is_empty());
    }

    #[test]
    fn collect_notifications_skips_pending() {
        let tmp = std::env::temp_dir().join(format!("oxi-notif2-{}", Uuid::new_v4()));
        let mut mgr = new_with_dir(tmp);
        mgr.create_task(TaskType::LocalBash {
            command: "sleep 10".into(),
        });
        // Task is still Pending — no notification expected.
        let notifs = collect_notifications(&mgr);
        assert!(notifs.is_empty());
    }
}
