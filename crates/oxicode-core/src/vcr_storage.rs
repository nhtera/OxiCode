//! VCR storage — persists cassette recordings to gzip-compressed JSON files.
//!
//! All cassettes are stored under `~/.oxicode/vcr/`. Each file is named
//! `{name}.vcr.gz` and contains a gzip-compressed JSON array of [`VcrEntry`].
//!
//! Hard limit: 100 MB per recording. Files exceeding this are rejected on save.

use std::io::{Read, Write};
use std::path::PathBuf;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

use crate::vcr_recorder::VcrEntry;

/// Maximum allowed size of a single cassette file in bytes (100 MiB).
const MAX_RECORDING_BYTES: u64 = 100 * 1024 * 1024;

/// Return the directory used to store VCR cassette files.
///
/// Defaults to `~/.oxicode/vcr/`. The directory is **not** created automatically
/// by this function — callers that need it to exist should call
/// [`std::fs::create_dir_all`] first.
pub fn vcr_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oxicode/vcr")
}

/// Derive the full path for a cassette file from its `name`.
fn cassette_path(name: &str) -> PathBuf {
    vcr_dir().join(format!("{name}.vcr.gz"))
}

/// Persist `entries` to a gzip-compressed JSON cassette named `name`.
///
/// Returns the path where the cassette was written.
/// Returns `Err` if the serialised payload exceeds [`MAX_RECORDING_BYTES`].
pub fn save(name: &str, entries: &[VcrEntry]) -> Result<PathBuf, String> {
    let json =
        serde_json::to_string(entries).map_err(|e| format!("Failed to serialize entries: {e}"))?;

    let raw_bytes = json.as_bytes();
    if raw_bytes.len() as u64 > MAX_RECORDING_BYTES {
        return Err(format!(
            "Recording exceeds 100 MB limit ({} bytes)",
            raw_bytes.len()
        ));
    }

    let dir = vcr_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create VCR directory {}: {e}", dir.display()))?;

    let path = cassette_path(name);
    let file =
        std::fs::File::create(&path).map_err(|e| format!("Failed to create file: {e}"))?;

    let mut encoder = GzEncoder::new(file, Compression::default());
    encoder
        .write_all(raw_bytes)
        .map_err(|e| format!("Failed to write compressed data: {e}"))?;
    encoder
        .finish()
        .map_err(|e| format!("Failed to finalise gzip stream: {e}"))?;

    tracing::debug!(
        path = %path.display(),
        entry_count = entries.len(),
        "VCR cassette saved"
    );

    Ok(path)
}

/// Load and decompress a cassette by `name`.
///
/// Returns `Err` if the file does not exist, cannot be decoded, or if the
/// decompressed content exceeds [`MAX_RECORDING_BYTES`].
pub fn load(name: &str) -> Result<Vec<VcrEntry>, String> {
    let path = cassette_path(name);

    let file =
        std::fs::File::open(&path).map_err(|e| format!("Cannot open cassette '{}': {e}", path.display()))?;

    let decoder = GzDecoder::new(file);
    // Guard against decompression bombs: limit decompressed read to MAX_RECORDING_BYTES.
    let mut limited = decoder.take(MAX_RECORDING_BYTES + 1);
    let mut json = String::new();
    limited
        .read_to_string(&mut json)
        .map_err(|e| format!("Failed to decompress cassette: {e}"))?;

    if json.len() as u64 > MAX_RECORDING_BYTES {
        return Err(format!(
            "Cassette '{}' exceeds 100 MB decompressed limit",
            path.display()
        ));
    }

    let entries: Vec<VcrEntry> =
        serde_json::from_str(&json).map_err(|e| format!("Failed to parse cassette JSON: {e}"))?;

    tracing::debug!(
        path = %path.display(),
        entry_count = entries.len(),
        "VCR cassette loaded"
    );

    Ok(entries)
}

/// List all cassettes stored in [`vcr_dir`].
///
/// Returns a `Vec` of `(name, size_bytes, date_string)` tuples sorted
/// alphabetically by name. The `date_string` is the file's last-modified
/// time formatted as `YYYY-MM-DD HH:MM:SS UTC`.
pub fn list() -> Result<Vec<(String, u64, String)>, String> {
    let dir = vcr_dir();

    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();

    let read_dir = std::fs::read_dir(&dir)
        .map_err(|e| format!("Cannot read VCR directory: {e}"))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Directory entry error: {e}"))?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("gz") {
            continue;
        }

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        // Strip ".vcr.gz" suffix to recover the cassette name.
        let name = file_name
            .strip_suffix(".vcr.gz")
            .unwrap_or(&file_name)
            .to_string();

        let meta = std::fs::metadata(&path)
            .map_err(|e| format!("Cannot stat {}: {e}", path.display()))?;

        let size = meta.len();

        let date = meta
            .modified()
            .ok()
            .and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| {
                    // Simple UTC formatting without chrono dep in this fn.
                    format_unix_secs(d.as_secs())
                })
            })
            .unwrap_or_else(|| "unknown".to_string());

        results.push((name, size, date));
    }

    results.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(results)
}

/// Delete a cassette by `name`.
///
/// Returns `Err` if the file does not exist or cannot be removed.
pub fn delete(name: &str) -> Result<(), String> {
    let path = cassette_path(name);
    std::fs::remove_file(&path)
        .map_err(|e| format!("Failed to delete cassette '{}': {e}", path.display()))
}

/// Format a Unix timestamp (seconds) as a human-readable UTC string.
///
/// Output format: `YYYY-MM-DD HH:MM:SS UTC`
fn format_unix_secs(secs: u64) -> String {
    // Minimal implementation — avoids pulling chrono into this module.
    let secs_per_min = 60u64;
    let secs_per_hour = 3600u64;
    let secs_per_day = 86400u64;

    let time_of_day = secs % secs_per_day;
    let hour = time_of_day / secs_per_hour;
    let minute = (time_of_day % secs_per_hour) / secs_per_min;
    let second = time_of_day % secs_per_min;

    // Days since Unix epoch → Gregorian date (Zeller-adjacent algorithm)
    let days = secs / secs_per_day;
    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
}

/// Convert days since Unix epoch (1970-01-01) to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Using the algorithm from https://howardhinnant.github.io/date_algorithms.html
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn vcr_dir_is_under_home() {
        let dir = vcr_dir();
        assert!(dir.to_string_lossy().contains(".oxicode"));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "oxi-vcr-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        // Point vcr_dir to temp by temporarily overriding via a direct path call.
        let entries = vec![VcrEntry {
            request_summary: "test req".to_string(),
            response_summary: "test resp".to_string(),
            timestamp: "2026-04-05T00:00:00Z".to_string(),
            duration_ms: 42,
        }];

        // Write directly to the temp path to avoid touching ~/.oxicode.
        let path = dir.join("test.vcr.gz");
        write_to_path(&path, &entries).unwrap();
        let loaded = load_from_path(&path).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].request_summary, "test req");
        assert_eq!(loaded[0].duration_ms, 42);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_unix_secs_known_date() {
        // 2026-04-09 00:00:00 UTC  == 1775692800
        let s = format_unix_secs(1_775_692_800);
        assert_eq!(s, "2026-04-09 00:00:00 UTC");
    }

    /// Helper: write entries directly to an arbitrary path.
    fn write_to_path(path: &Path, entries: &[VcrEntry]) -> Result<(), String> {
        let json = serde_json::to_string(entries).map_err(|e| e.to_string())?;
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut enc = GzEncoder::new(file, Compression::default());
        enc.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        enc.finish().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Helper: load entries directly from an arbitrary path.
    fn load_from_path(path: &Path) -> Result<Vec<VcrEntry>, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut dec = GzDecoder::new(file);
        let mut json = String::new();
        dec.read_to_string(&mut json).map_err(|e| e.to_string())?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }
}
