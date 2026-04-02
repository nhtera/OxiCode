//! `oxicode-tasks` — background task management.
//!
//! Provides:
//! - [`manager::TaskManager`] — in-process task registry
//! - [`runner`] — async process spawning with disk-based output streaming
//! - [`output::OutputReader`] — incremental JSONL reader
//! - [`notifications::NotificationCollector`] — de-duplicating notification emitter

pub mod manager;
pub mod notifications;
pub mod output;
pub mod runner;

// Key type re-exports for callers who only need the surface API.
pub use manager::{TaskEntry, TaskManager, TaskStatus, TaskType};
pub use notifications::{
    collect_notifications, format_notification, NotificationCollector, TaskNotification,
};
pub use output::{read_all, OutputLine, OutputReader, OutputStream};
