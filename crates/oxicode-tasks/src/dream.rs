//! Dream task — proactive background agent that periodically checks conditions.
//!
//! Wakes on a configurable interval, evaluates trigger conditions (file changes,
//! time-based, user idle), and spawns an agent when triggered. Rate-limited to
//! prevent notification spam.

use std::path::Path;
use std::time::{Duration, Instant};

use oxicode_common::OxiResult;
use serde::{Deserialize, Serialize};

use crate::manager::TaskStatus;
use crate::task_output_helpers::{open_output_file, write_line};

/// Default wake interval in seconds (5 minutes).
const DEFAULT_WAKE_INTERVAL_SECS: u64 = 300;

/// Minimum time between triggered actions (rate limit).
const MIN_TRIGGER_INTERVAL_SECS: u64 = 60;

/// Maximum number of triggers before the dream task auto-stops.
const MAX_TRIGGERS: u32 = 50;

/// Condition that causes the dream task to fire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DreamCondition {
    /// Fire when any file matching the glob has changed since last check.
    FileChanged { glob_pattern: String },
    /// Fire on a time schedule (e.g. every N wakes).
    Periodic { every_n_wakes: u32 },
    /// Fire when the user has been idle for at least N seconds.
    UserIdle { idle_threshold_secs: u64 },
}

/// Configuration for a dream task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamConfig {
    /// How often to wake and check conditions (seconds).
    pub wake_interval_secs: u64,
    /// Condition(s) to evaluate on each wake.
    pub conditions: Vec<DreamCondition>,
    /// The prompt to pass to the spawned agent when triggered.
    pub prompt: String,
    /// Model for the spawned agent.
    pub model: String,
}

impl Default for DreamConfig {
    fn default() -> Self {
        Self {
            wake_interval_secs: DEFAULT_WAKE_INTERVAL_SECS,
            conditions: vec![DreamCondition::Periodic { every_n_wakes: 1 }],
            prompt: String::new(),
            model: String::new(),
        }
    }
}

/// Evaluate whether any condition is met on this wake cycle.
fn evaluate_conditions(
    conditions: &[DreamCondition],
    wake_count: u32,
    _last_user_activity: Option<Instant>,
) -> bool {
    for cond in conditions {
        match cond {
            DreamCondition::FileChanged { glob_pattern } => {
                // Check if any matching file was modified in the last wake interval.
                if let Ok(paths) = glob::glob(glob_pattern) {
                    for entry in paths.flatten() {
                        if let Ok(meta) = entry.metadata() {
                            if let Ok(modified) = meta.modified() {
                                if modified.elapsed().unwrap_or(Duration::MAX)
                                    < Duration::from_secs(DEFAULT_WAKE_INTERVAL_SECS)
                                {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
            DreamCondition::Periodic { every_n_wakes } => {
                if *every_n_wakes > 0 && wake_count % every_n_wakes == 0 {
                    return true;
                }
            }
            DreamCondition::UserIdle {
                idle_threshold_secs,
            } => {
                if let Some(last_activity) = _last_user_activity {
                    if last_activity.elapsed() >= Duration::from_secs(*idle_threshold_secs) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Run the dream task loop — wakes periodically, checks conditions, logs triggers.
///
/// In production, the "trigger" action would spawn an agent task. Here we log the
/// trigger event and the prompt so the caller (task manager) can act on it.
///
/// The loop runs until cancelled via the `cancel` token or `MAX_TRIGGERS` is reached.
pub async fn run_dream(
    task_id: &str,
    config: &DreamConfig,
    cancel: tokio::sync::watch::Receiver<bool>,
    tasks_dir: &Path,
) -> OxiResult<TaskStatus> {
    tracing::info!(
        "run_dream task={} interval={}s conditions={}",
        task_id,
        config.wake_interval_secs,
        config.conditions.len()
    );

    let mut out_file = open_output_file(tasks_dir, task_id)?;
    let interval = Duration::from_secs(config.wake_interval_secs);
    let rate_limit = Duration::from_secs(MIN_TRIGGER_INTERVAL_SECS);

    let mut wake_count: u32 = 0;
    let mut trigger_count: u32 = 0;
    let mut last_trigger = Instant::now() - rate_limit; // allow immediate first trigger

    write_line(
        &mut out_file,
        "stdout",
        &format!(
            "dream task started: interval={}s, conditions={}",
            config.wake_interval_secs,
            config.conditions.len()
        ),
    )?;

    loop {
        // Sleep for the wake interval, checking cancel token.
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = cancel_wait(cancel.clone()) => {
                write_line(&mut out_file, "stdout", "dream task cancelled")?;
                tracing::info!("dream task={} cancelled", task_id);
                return Ok(TaskStatus::Killed);
            }
        }

        wake_count = wake_count.saturating_add(1);
        tracing::debug!("dream task={} wake #{}", task_id, wake_count);

        // Evaluate conditions.
        if evaluate_conditions(&config.conditions, wake_count, None) {
            // Rate limit check.
            if last_trigger.elapsed() < rate_limit {
                write_line(&mut out_file, "stderr", "trigger suppressed by rate limit")?;
                continue;
            }

            trigger_count += 1;
            last_trigger = Instant::now();

            write_line(
                &mut out_file,
                "stdout",
                &format!(
                    "triggered #{trigger_count}: prompt={:?} model={}",
                    truncate(&config.prompt, 80),
                    config.model
                ),
            )?;

            tracing::info!(
                "dream task={} trigger #{} prompt={:?}",
                task_id,
                trigger_count,
                truncate(&config.prompt, 50)
            );

            if trigger_count >= MAX_TRIGGERS {
                write_line(
                    &mut out_file,
                    "stdout",
                    &format!("max triggers ({MAX_TRIGGERS}) reached, stopping"),
                )?;
                return Ok(TaskStatus::Completed { exit_code: 0 });
            }
        }
    }
}

/// Wait until the cancel token becomes `true`.
async fn cancel_wait(mut rx: tokio::sync::watch::Receiver<bool>) {
    while !*rx.borrow() {
        if rx.changed().await.is_err() {
            // Sender dropped — treat as cancel.
            return;
        }
    }
}

/// Truncate a string for log display (UTF-8 safe).
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        // Walk back to a char boundary to avoid panicking on multi-byte UTF-8.
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn tmp_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("oxi-dream-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn periodic_condition_fires_on_schedule() {
        // every_n_wakes=3 should fire on wake 3, 6, 9...
        let conds = vec![DreamCondition::Periodic { every_n_wakes: 3 }];
        assert!(!evaluate_conditions(&conds, 1, None));
        assert!(!evaluate_conditions(&conds, 2, None));
        assert!(evaluate_conditions(&conds, 3, None));
        assert!(!evaluate_conditions(&conds, 4, None));
        assert!(evaluate_conditions(&conds, 6, None));
    }

    #[test]
    fn user_idle_condition() {
        let conds = vec![DreamCondition::UserIdle {
            idle_threshold_secs: 1,
        }];
        // Idle for 2 seconds — should trigger.
        let last_activity = Instant::now() - Duration::from_secs(2);
        assert!(evaluate_conditions(&conds, 1, Some(last_activity)));

        // Active just now — should not trigger.
        let recent = Instant::now();
        assert!(!evaluate_conditions(&conds, 1, Some(recent)));
    }

    #[test]
    fn default_config() {
        let config = DreamConfig::default();
        assert_eq!(config.wake_interval_secs, DEFAULT_WAKE_INTERVAL_SECS);
        assert_eq!(config.conditions.len(), 1);
    }

    #[tokio::test]
    async fn dream_task_cancellation() {
        let dir = tmp_dir();
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

        let config = DreamConfig {
            wake_interval_secs: 60, // long interval
            conditions: vec![DreamCondition::Periodic { every_n_wakes: 1 }],
            prompt: "check status".into(),
            model: "test".into(),
        };

        // Cancel immediately.
        cancel_tx.send(true).unwrap();

        let status = run_dream("t-dream-1", &config, cancel_rx, &dir)
            .await
            .unwrap();
        assert!(matches!(status, TaskStatus::Killed));
    }

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello");
    }
}
