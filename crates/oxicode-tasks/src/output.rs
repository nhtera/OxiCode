use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use oxicode_common::{OxiError, OxiResult};
use serde::{Deserialize, Serialize};

/// Which output stream a line originated from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// A single parsed line from a task's `output.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputLine {
    #[serde(rename = "ts")]
    pub timestamp: DateTime<Utc>,
    pub stream: OutputStream,
    pub line: String,
}

/// Raw shape of one JSONL record written by the runner.
#[derive(Deserialize)]
struct RawRecord {
    ts: DateTime<Utc>,
    stream: OutputStream,
    line: String,
}

impl From<RawRecord> for OutputLine {
    fn from(r: RawRecord) -> Self {
        Self {
            timestamp: r.ts,
            stream: r.stream,
            line: r.line,
        }
    }
}

/// Incremental reader for a task's `output.jsonl`.
/// Tracks a byte offset so repeated calls return only new lines.
#[derive(Debug)]
pub struct OutputReader {
    pub task_id: String,
    pub tasks_dir: PathBuf,
    /// Byte offset into the file — advances after each `read_new_lines`.
    offset: u64,
}

impl OutputReader {
    pub fn new(task_id: String, tasks_dir: PathBuf) -> Self {
        Self {
            task_id,
            tasks_dir,
            offset: 0,
        }
    }

    fn output_path(&self) -> PathBuf {
        self.tasks_dir
            .join(&self.task_id)
            .join("output.jsonl")
    }

    /// Return any lines written since the last call, advancing the internal offset.
    /// Returns an empty vec if there are no new lines or the file does not yet exist.
    pub fn read_new_lines(&mut self) -> OxiResult<Vec<OutputLine>> {
        let path = self.output_path();
        if !path.exists() {
            return Ok(vec![]);
        }

        let mut file = std::fs::File::open(&path)?;
        file.seek(SeekFrom::Start(self.offset))?;

        let mut lines = Vec::new();
        let mut reader = BufReader::new(&mut file);
        let mut raw = String::new();

        loop {
            raw.clear();
            let n = reader.read_line(&mut raw)?;
            if n == 0 {
                break;
            }
            let trimmed = raw.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            let record: RawRecord = serde_json::from_str(trimmed)
                .map_err(|e| OxiError::Other(format!("bad jsonl line: {e}")))?;
            lines.push(record.into());
        }

        // Persist new offset.
        self.offset = file.stream_position()?;
        tracing::debug!(
            "read_new_lines task={} new={} offset={}",
            self.task_id,
            lines.len(),
            self.offset
        );
        Ok(lines)
    }
}

/// Read every line from a task's output file in one shot (no state).
pub fn read_all(task_id: &str, tasks_dir: &Path) -> OxiResult<Vec<OutputLine>> {
    let path = tasks_dir.join(task_id).join("output.jsonl");
    if !path.exists() {
        return Ok(vec![]);
    }

    let file = std::fs::File::open(&path)?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();

    for (i, raw) in reader.lines().enumerate() {
        let raw = raw?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let record: RawRecord = serde_json::from_str(trimmed)
            .map_err(|e| OxiError::Other(format!("line {i}: bad jsonl: {e}")))?;
        lines.push(record.into());
    }

    tracing::debug!("read_all task={} total={}", task_id, lines.len());
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use uuid::Uuid;

    fn write_jsonl(dir: &Path, task_id: &str, entries: &[(&str, &str)]) {
        let task_dir = dir.join(task_id);
        std::fs::create_dir_all(&task_dir).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(task_dir.join("output.jsonl"))
            .unwrap();
        for (stream, line) in entries {
            let ts = Utc::now().to_rfc3339();
            writeln!(f, r#"{{"ts":"{ts}","stream":"{stream}","line":"{line}"}}"#).unwrap();
        }
    }

    fn tmp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("oxi-output-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn read_all_parses_lines() {
        let dir = tmp_dir();
        write_jsonl(&dir, "t1", &[("stdout", "hello"), ("stderr", "warn")]);
        let lines = read_all("t1", &dir).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line, "hello");
        assert_eq!(lines[1].stream, OutputStream::Stderr);
    }

    #[test]
    fn incremental_reader_tracks_offset() {
        let dir = tmp_dir();
        write_jsonl(&dir, "t2", &[("stdout", "first")]);

        let mut reader = OutputReader::new("t2".into(), dir.clone());
        let batch1 = reader.read_new_lines().unwrap();
        assert_eq!(batch1.len(), 1);

        // No new lines yet.
        let batch2 = reader.read_new_lines().unwrap();
        assert_eq!(batch2.len(), 0);

        // Append a second line.
        write_jsonl(&dir, "t2", &[("stdout", "second")]);
        let batch3 = reader.read_new_lines().unwrap();
        assert_eq!(batch3.len(), 1);
        assert_eq!(batch3[0].line, "second");
    }

    #[test]
    fn missing_file_returns_empty() {
        let dir = tmp_dir();
        let lines = read_all("no_such_task", &dir).unwrap();
        assert!(lines.is_empty());
    }
}
