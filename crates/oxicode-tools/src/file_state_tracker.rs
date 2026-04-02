//! Tracks file modification times to detect external changes between read and edit.
//!
//! When a file is read, its mtime is recorded. Before an edit or write, the current
//! mtime is compared against the recorded one. If it changed, the edit is rejected
//! to prevent silent data loss.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::SystemTime;

/// Shared file state tracker for detecting stale edits.
#[derive(Debug, Default)]
pub struct FileStateTracker {
    /// Maps canonical file paths to their mtime at last read.
    state: RwLock<HashMap<PathBuf, SystemTime>>,
}

impl FileStateTracker {
    /// Record the current mtime for a file (called after reading).
    pub fn record(&self, path: &Path) {
        if let Ok(meta) = std::fs::metadata(path) {
            if let Ok(mtime) = meta.modified() {
                let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                self.state.write().unwrap().insert(key, mtime);
            }
        }
    }

    /// Check if a file was modified since the last recorded read.
    /// Returns `Ok(())` if safe to write, `Err(message)` if stale.
    pub fn check_staleness(&self, path: &Path) -> Result<(), String> {
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let guard = self.state.read().unwrap();
        let Some(recorded_mtime) = guard.get(&key) else {
            // File was never read via FileReadTool — allow the edit but warn
            return Ok(());
        };

        let current_mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .map_err(|e| format!("Cannot stat {}: {e}", path.display()))?;

        if current_mtime != *recorded_mtime {
            return Err(format!(
                "File {} was modified externally since last read. \
                 Re-read the file before editing to avoid data loss.",
                path.display()
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_record_and_check_no_change() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "content").unwrap();

        let tracker = FileStateTracker::default();
        tracker.record(f.path());
        assert!(tracker.check_staleness(f.path()).is_ok());
    }

    #[test]
    fn test_detects_external_modification() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "original").unwrap();

        let tracker = FileStateTracker::default();
        tracker.record(f.path());

        // Simulate external modification
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(f.path(), "modified").unwrap();

        assert!(tracker.check_staleness(f.path()).is_err());
    }

    #[test]
    fn test_untracked_file_allowed() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "content").unwrap();

        let tracker = FileStateTracker::default();
        // Never recorded — should still allow
        assert!(tracker.check_staleness(f.path()).is_ok());
    }
}
