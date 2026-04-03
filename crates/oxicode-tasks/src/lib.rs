//! `oxicode-tasks` — background task management.
//!
//! Provides:
//! - [`manager::TaskManager`] — in-process task registry
//! - [`runner`] — async process spawning with disk-based output streaming
//! - [`output::OutputReader`] — incremental JSONL reader
//! - [`notifications::NotificationCollector`] — de-duplicating notification emitter
//! - [`remote_agent`] — remote agent task (feature-gated: `remote`)
//! - [`teammate`] — in-process teammate task (feature-gated: `teammate`)
//! - [`dream`] — proactive background agent (feature-gated: `dream`)

pub mod manager;
pub mod notifications;
pub mod output;
pub mod runner;
pub mod task_output_helpers;

#[cfg(feature = "remote")]
pub mod remote_agent;

#[cfg(feature = "teammate")]
pub mod teammate;

#[cfg(feature = "dream")]
pub mod dream;

// Key type re-exports for callers who only need the surface API.
pub use manager::{TaskEntry, TaskManager, TaskStatus, TaskType};
pub use notifications::{
    collect_notifications, format_notification, NotificationCollector, TaskNotification,
};
pub use output::{read_all, OutputLine, OutputReader, OutputStream};
