//! Shared output helpers for task runners.
//!
//! Used by all task types to write JSONL output to disk.

use std::io::Write as _;
use std::path::Path;

use chrono::Utc;
use oxicode_common::OxiResult;
use serde_json::json;

/// Open (and optionally create) the output JSONL file for a task.
pub fn open_output_file(tasks_dir: &Path, task_id: &str) -> OxiResult<std::fs::File> {
    let dir = tasks_dir.join(task_id);
    std::fs::create_dir_all(&dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("output.jsonl"))?;
    Ok(file)
}

/// Append one JSON line to the output file.
pub fn write_line(file: &mut std::fs::File, stream: &str, line: &str) -> OxiResult<()> {
    let record = json!({
        "ts": Utc::now().to_rfc3339(),
        "stream": stream,
        "line": line,
    });
    writeln!(file, "{record}")?;
    Ok(())
}
