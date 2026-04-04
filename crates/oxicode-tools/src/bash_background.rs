use std::path::Path;
use std::sync::{Arc, Mutex};

use oxicode_tasks::{TaskManager, TaskStatus, TaskType};

/// Manages background bash command execution with task system integration.
///
/// Spawns commands as tokio tasks, registers them with `TaskManager`,
/// streams output to disk, and returns the task ID immediately.
pub struct BackgroundRunner {
    task_manager: Arc<Mutex<TaskManager>>,
    task_abort_handles: Arc<Mutex<std::collections::HashMap<String, tokio::task::AbortHandle>>>,
}

impl BackgroundRunner {
    pub fn new(
        task_manager: Arc<Mutex<TaskManager>>,
        task_abort_handles: Arc<Mutex<std::collections::HashMap<String, tokio::task::AbortHandle>>>,
    ) -> Self {
        Self {
            task_manager,
            task_abort_handles,
        }
    }

    /// Spawn a command in the background and return its task ID immediately.
    ///
    /// The command runs in a separate tokio task. Output streams to
    /// `~/.oxicode/tasks/{id}/output.jsonl`. The task status is updated
    /// in `TaskManager` on completion.
    pub fn spawn(&self, command: &str, working_dir: &Path) -> String {
        let task_id = {
            let mut mgr = self.task_manager.lock().expect("lock task manager");
            let id = mgr.create_task(TaskType::LocalBash {
                command: command.to_string(),
            });
            mgr.update_status(&id, TaskStatus::Running);
            id
        };

        let tasks_dir = {
            let mgr = self.task_manager.lock().expect("lock task manager");
            mgr.tasks_dir.clone()
        };

        let cmd = command.to_string();
        let wd = working_dir.to_path_buf();
        let tm = Arc::clone(&self.task_manager);
        let tid = task_id.clone();

        let handle = tokio::spawn(async move {
            let status = run_background_command(&tid, &cmd, &wd, &tasks_dir).await;
            let mut mgr = tm.lock().expect("lock task manager");
            mgr.update_status(&tid, status);
        });

        // Store abort handle so TaskStop can cancel the task.
        {
            let mut handles = self.task_abort_handles.lock().expect("lock abort handles");
            handles.insert(task_id.clone(), handle.abort_handle());
        }

        task_id
    }

    /// Get the output file path for a background task.
    pub fn output_path(&self, task_id: &str) -> std::path::PathBuf {
        let mgr = self.task_manager.lock().expect("lock task manager");
        mgr.tasks_dir.join(task_id).join("output.jsonl")
    }
}

/// Run a command in the background, streaming output to disk.
///
/// Prepends `cd <working_dir>` to ensure the background command runs in the
/// same directory as foreground commands would. This is necessary because
/// `run_bash` in oxicode-tasks doesn't accept a working_dir parameter.
async fn run_background_command(
    task_id: &str,
    command: &str,
    working_dir: &Path,
    tasks_dir: &Path,
) -> TaskStatus {
    // Wrap command with cd to ensure correct working directory.
    let wrapped_command = format!(
        "cd {} && {{ {}; }}",
        shell_escape(working_dir.to_string_lossy().as_ref()),
        command
    );

    match oxicode_tasks::runner::run_bash(task_id, &wrapped_command, tasks_dir).await {
        Ok(status) => {
            tracing::info!("Background task {} completed: {:?}", task_id, status);
            status
        }
        Err(e) => {
            tracing::error!("Background task {} failed: {}", task_id, e);
            TaskStatus::Failed {
                error: e.to_string(),
            }
        }
    }
}

/// Escape a string for safe use in a shell command (single-quote wrapping).
fn shell_escape(s: &str) -> String {
    // Replace single quotes with '\'' (end quote, escaped quote, restart quote)
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_runner() -> (BackgroundRunner, Arc<Mutex<TaskManager>>) {
        // TaskManager::new() creates ~/.oxicode/tasks/ by default, which is fine for tests.
        let tm = Arc::new(Mutex::new(TaskManager::default()));
        let handles = Arc::new(Mutex::new(HashMap::new()));
        let runner = BackgroundRunner::new(Arc::clone(&tm), handles);
        (runner, tm)
    }

    #[tokio::test]
    async fn spawn_returns_task_id() {
        let (runner, tm) = make_runner();
        let id = runner.spawn("echo background-test", Path::new("/tmp"));
        assert!(!id.is_empty());

        // Task should exist in manager
        let mgr = tm.lock().unwrap();
        let entry = mgr.get_task(&id);
        assert!(entry.is_some());
    }

    #[tokio::test]
    async fn background_task_completes() {
        let (runner, tm) = make_runner();
        let id = runner.spawn("echo done", Path::new("/tmp"));

        // Wait a bit for the background task to finish
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let mgr = tm.lock().unwrap();
        let entry = mgr.get_task(&id).expect("task should exist");
        assert!(
            matches!(entry.status, TaskStatus::Completed { .. }),
            "expected Completed, got {:?}",
            entry.status
        );
    }

    #[tokio::test]
    async fn background_task_nonzero_exit() {
        let (runner, tm) = make_runner();
        let id = runner.spawn("exit 42", Path::new("/tmp"));

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let mgr = tm.lock().unwrap();
        let entry = mgr.get_task(&id).expect("task should exist");
        assert!(
            matches!(entry.status, TaskStatus::Failed { .. }),
            "expected Failed, got {:?}",
            entry.status
        );
    }

    #[tokio::test]
    async fn output_path_correct() {
        let (runner, _) = make_runner();
        let id = runner.spawn("echo test", Path::new("/tmp"));
        let path = runner.output_path(&id);
        assert!(path.to_string_lossy().contains(&id));
        assert!(path.to_string_lossy().ends_with("output.jsonl"));
    }
}
