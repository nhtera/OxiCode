use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Type of background task to run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskType {
    LocalBash {
        command: String,
    },
    LocalAgent {
        prompt: String,
        model: String,
    },
    Monitor {
        interval_secs: u64,
        command: String,
    },
    #[cfg(feature = "remote")]
    RemoteAgent {
        server_url: String,
        prompt: String,
        model: String,
    },
    #[cfg(feature = "teammate")]
    InProcessTeammate {
        name: String,
        prompt: String,
        owned_files: Vec<String>,
    },
    #[cfg(feature = "dream")]
    Dream {
        prompt: String,
        model: String,
        wake_interval_secs: u64,
    },
}

/// Lifecycle status of a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed { exit_code: i32 },
    Failed { error: String },
    Killed,
}

/// A single registered task entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEntry {
    pub id: String,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Central in-process registry for background tasks.
#[derive(Debug)]
pub struct TaskManager {
    tasks: HashMap<String, TaskEntry>,
    pub tasks_dir: PathBuf,
}

impl TaskManager {
    /// Create manager, ensuring `~/.oxicode/tasks/` exists.
    pub fn new() -> Self {
        let tasks_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".oxicode/tasks");

        if let Err(e) = std::fs::create_dir_all(&tasks_dir) {
            tracing::warn!("Could not create tasks dir {:?}: {}", tasks_dir, e);
        }

        tracing::debug!("TaskManager initialised, tasks_dir={:?}", tasks_dir);
        Self {
            tasks: HashMap::new(),
            tasks_dir,
        }
    }

    /// Register a new task and return its generated ID.
    pub fn create_task(&mut self, task_type: TaskType) -> String {
        let id = Uuid::new_v4().to_string();
        let entry = TaskEntry {
            id: id.clone(),
            task_type,
            status: TaskStatus::Pending,
            created_at: Utc::now(),
            completed_at: None,
        };
        tracing::info!("Task created id={}", id);
        self.tasks.insert(id.clone(), entry);
        id
    }

    pub fn get_task(&self, id: &str) -> Option<&TaskEntry> {
        self.tasks.get(id)
    }

    pub fn list_tasks(&self) -> Vec<&TaskEntry> {
        self.tasks.values().collect()
    }

    /// Update status (and set `completed_at` for terminal states).
    pub fn update_status(&mut self, id: &str, status: TaskStatus) {
        if let Some(entry) = self.tasks.get_mut(id) {
            let terminal = matches!(
                &status,
                TaskStatus::Completed { .. } | TaskStatus::Failed { .. } | TaskStatus::Killed
            );
            entry.status = status;
            if terminal {
                entry.completed_at = Some(Utc::now());
            }
            tracing::debug!("Task {} status updated", id);
        }
    }

    pub fn remove_task(&mut self, id: &str) {
        self.tasks.remove(id);
        tracing::debug!("Task {} removed", id);
    }

    /// Kill a running task by ID. Verifies the task exists and is Running,
    /// then updates status to Killed. The caller is responsible for actually
    /// terminating the process (e.g., via signal).
    pub fn kill_task(&mut self, id: &str) -> Result<(), String> {
        let entry = self
            .tasks
            .get(id)
            .ok_or_else(|| format!("Task '{id}' not found"))?;

        if !matches!(entry.status, TaskStatus::Running) {
            return Err(format!(
                "Task '{id}' is not running (status: {:?})",
                entry.status
            ));
        }

        self.update_status(id, TaskStatus::Killed);
        tracing::info!("Task {} killed", id);
        Ok(())
    }

    /// Drop all completed/failed/killed tasks older than 1 hour.
    pub fn cleanup_completed(&mut self) {
        let cutoff = Utc::now() - chrono::Duration::hours(1);
        self.tasks.retain(|_, entry| {
            let is_terminal = matches!(
                &entry.status,
                TaskStatus::Completed { .. } | TaskStatus::Failed { .. } | TaskStatus::Killed
            );
            if is_terminal {
                if let Some(completed_at) = entry.completed_at {
                    return completed_at > cutoff;
                }
            }
            true
        });
        tracing::info!("cleanup_completed finished");
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a `TaskManager` pointed at a custom directory — used in tests only.
#[cfg(test)]
pub fn new_with_dir(tasks_dir: std::path::PathBuf) -> TaskManager {
    std::fs::create_dir_all(&tasks_dir).ok();
    TaskManager {
        tasks: HashMap::new(),
        tasks_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> TaskManager {
        let tmp = std::env::temp_dir().join(format!("oxi-test-{}", Uuid::new_v4()));
        new_with_dir(tmp)
    }

    #[test]
    fn create_and_get_task() {
        let mut mgr = make_manager();
        let id = mgr.create_task(TaskType::LocalBash {
            command: "echo hi".into(),
        });
        let entry = mgr.get_task(&id).expect("task should exist");
        assert_eq!(entry.id, id);
        assert!(matches!(entry.status, TaskStatus::Pending));
    }

    #[test]
    fn cleanup_removes_old_completed_tasks() {
        let mut mgr = make_manager();
        let id = mgr.create_task(TaskType::LocalBash {
            command: "true".into(),
        });
        mgr.update_status(&id, TaskStatus::Completed { exit_code: 0 });
        // Manually backdate completed_at by 2 hours.
        if let Some(e) = mgr.tasks.get_mut(&id) {
            e.completed_at = Some(Utc::now() - chrono::Duration::hours(2));
        }
        mgr.cleanup_completed();
        assert!(mgr.get_task(&id).is_none(), "old task should be removed");
    }

    #[test]
    fn cleanup_retains_recent_completed_tasks() {
        let mut mgr = make_manager();
        let id = mgr.create_task(TaskType::LocalBash {
            command: "true".into(),
        });
        mgr.update_status(&id, TaskStatus::Completed { exit_code: 0 });
        mgr.cleanup_completed();
        assert!(mgr.get_task(&id).is_some(), "recent task should be kept");
    }

    #[test]
    fn kill_running_task() {
        let mut mgr = make_manager();
        let id = mgr.create_task(TaskType::LocalBash {
            command: "sleep 60".into(),
        });
        mgr.update_status(&id, TaskStatus::Running);

        let result = mgr.kill_task(&id);
        assert!(result.is_ok());
        assert!(matches!(
            mgr.get_task(&id).unwrap().status,
            TaskStatus::Killed
        ));
    }

    #[test]
    fn kill_non_running_task_fails() {
        let mut mgr = make_manager();
        let id = mgr.create_task(TaskType::LocalBash {
            command: "true".into(),
        });
        // Still Pending, not Running.
        let result = mgr.kill_task(&id);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not running"));
    }

    #[test]
    fn kill_nonexistent_task_fails() {
        let mut mgr = make_manager();
        let result = mgr.kill_task("does-not-exist");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }
}
