//! Files API — multipart upload, streaming download, and file-spec parsing.
//!
//! Provides:
//! - Upload files via multipart/form-data (max 50 MB)
//! - Download session files with streaming (no full buffering)
//! - Parse file specs in `path:line` format
//! - Retry on 429/5xx with exponential backoff

use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum allowed file size for upload/download (50 MB).
const MAX_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// Maximum retry attempts on transient errors (429 / 5xx).
const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff.
const BASE_DELAY: Duration = Duration::from_secs(1);

/// Upload request timeout.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Response from a successful file upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileUploadResponse {
    pub id: String,
    pub url: String,
    pub size: u64,
}

/// A parsed file specification (`path:line`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSpec {
    pub path: PathBuf,
    pub line: Option<u32>,
}

/// Errors that can occur during Files API operations.
#[derive(Debug, thiserror::Error)]
pub enum FilesApiError {
    #[error("file not found: {0}")]
    FileNotFound(PathBuf),

    #[error("file exceeds {MAX_FILE_SIZE} byte limit: {0} bytes")]
    FileTooLarge(u64),

    #[error("path traversal blocked: {0}")]
    PathTraversal(String),

    #[error("HTTP error: {status} — {body}")]
    HttpError { status: u16, body: String },

    #[error("max retries ({MAX_RETRIES}) exceeded")]
    MaxRetriesExceeded,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("request error: {0}")]
    Request(#[from] reqwest::Error),
}

// ---------------------------------------------------------------------------
// Upload
// ---------------------------------------------------------------------------

/// Upload a file via multipart/form-data.
///
/// - Validates file exists and size < 50 MB
/// - Blocks path traversal (`..` in path)
/// - Retries on 429/5xx with exponential backoff (1s, 2s, 4s)
pub async fn upload_file(
    client: &Client,
    api_base: &str,
    auth: &str,
    path: &Path,
) -> Result<FileUploadResponse, FilesApiError> {
    // Validate path traversal.
    validate_no_traversal(path)?;

    // Validate file exists.
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|_| FilesApiError::FileNotFound(path.to_path_buf()))?;

    // Validate size.
    if metadata.len() > MAX_FILE_SIZE {
        return Err(FilesApiError::FileTooLarge(metadata.len()));
    }

    let file_bytes = tokio::fs::read(path).await?;
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let url = format!("{api_base}/files/upload");

    for attempt in 1..=MAX_RETRIES {
        let part = reqwest::multipart::Part::bytes(file_bytes.clone())
            .file_name(file_name.clone())
            .mime_str("application/octet-stream")
            .expect("static MIME type is always valid");

        let form = reqwest::multipart::Form::new().part("file", part);

        let resp = client
            .post(&url)
            .bearer_auth(auth)
            .multipart(form)
            .timeout(UPLOAD_TIMEOUT)
            .send()
            .await?;

        let status = resp.status().as_u16();

        if resp.status().is_success() {
            return resp
                .json::<FileUploadResponse>()
                .await
                .map_err(FilesApiError::Request);
        }

        if is_retryable(status) && attempt < MAX_RETRIES {
            let delay = backoff_delay(attempt);
            warn!(attempt, status, ?delay, "retrying upload");
            tokio::time::sleep(delay).await;
            continue;
        }

        let body = resp.text().await.unwrap_or_default();
        return Err(FilesApiError::HttpError { status, body });
    }

    Err(FilesApiError::MaxRetriesExceeded)
}

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

/// Download all files for a session, streaming each to `dest/`.
///
/// Returns the list of paths written on disk.
pub async fn download_session_files(
    client: &Client,
    api_base: &str,
    auth: &str,
    session_id: &str,
    dest: &Path,
) -> Result<Vec<PathBuf>, FilesApiError> {
    let list_url = format!("{api_base}/sessions/{session_id}/files");

    let resp = client
        .get(&list_url)
        .bearer_auth(auth)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(FilesApiError::HttpError { status, body });
    }

    let file_list: Vec<FileEntry> = resp.json().await.map_err(FilesApiError::Request)?;

    tokio::fs::create_dir_all(dest).await?;

    let mut downloaded = Vec::new();

    for entry in &file_list {
        let dest_path = dest.join(&entry.name);

        // Block path traversal in server-supplied names.
        validate_no_traversal(Path::new(&entry.name))?;
        // Block absolute path injection (join replaces base for absolute names).
        validate_within_dir(dest, &dest_path)?;

        let file_resp = client
            .get(&entry.download_url)
            .bearer_auth(auth)
            .send()
            .await?;

        if !file_resp.status().is_success() {
            let status = file_resp.status().as_u16();
            let body = file_resp.text().await.unwrap_or_default();
            return Err(FilesApiError::HttpError { status, body });
        }

        // Stream to disk.
        let mut file = tokio::fs::File::create(&dest_path).await?;
        let mut total: u64 = 0;
        let mut stream = file_resp.bytes_stream();

        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(FilesApiError::Request)?;
            total += chunk.len() as u64;
            if total > MAX_FILE_SIZE {
                // Clean up partial file.
                drop(file);
                let _ = tokio::fs::remove_file(&dest_path).await;
                return Err(FilesApiError::FileTooLarge(total));
            }
            file.write_all(&chunk).await?;
        }

        file.flush().await?;
        debug!(path = %dest_path.display(), bytes = total, "downloaded file");
        downloaded.push(dest_path);
    }

    Ok(downloaded)
}

/// Server-side file listing entry (minimal contract).
#[derive(Debug, Deserialize)]
struct FileEntry {
    name: String,
    download_url: String,
}

// ---------------------------------------------------------------------------
// File spec parsing
// ---------------------------------------------------------------------------

/// Parse a whitespace/comma-separated list of file specs.
///
/// Format: `"path/to/file.rs:42"` -> `FileSpec { path, line: Some(42) }`
///         `"path/to/file.rs"`    -> `FileSpec { path, line: None }`
pub fn parse_file_specs(specs: &str) -> Vec<FileSpec> {
    specs
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(parse_single_spec)
        .collect()
}

/// Parse a single `path:line` spec.
fn parse_single_spec(spec: &str) -> FileSpec {
    // Find last colon that is followed by digits only (avoids splitting on
    // Windows drive letters like `C:\foo`).
    if let Some(colon_pos) = spec.rfind(':') {
        let (path_part, line_part) = spec.split_at(colon_pos);
        let line_str = &line_part[1..]; // skip ':'
        if !path_part.is_empty() {
            if let Ok(line) = line_str.parse::<u32>() {
                return FileSpec {
                    path: PathBuf::from(path_part),
                    line: Some(line),
                };
            }
        }
    }
    FileSpec {
        path: PathBuf::from(spec),
        line: None,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Reject paths containing `..` components or absolute segments (path traversal).
fn validate_no_traversal(path: &Path) -> Result<(), FilesApiError> {
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                return Err(FilesApiError::PathTraversal(
                    path.display().to_string(),
                ));
            }
            std::path::Component::RootDir => {
                return Err(FilesApiError::PathTraversal(
                    path.display().to_string(),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validate that a resolved path stays within the expected parent directory.
/// Catches absolute path injection where `Path::join` replaces the base entirely.
fn validate_within_dir(dest: &Path, resolved: &Path) -> Result<(), FilesApiError> {
    if !resolved.starts_with(dest) {
        return Err(FilesApiError::PathTraversal(
            resolved.display().to_string(),
        ));
    }
    Ok(())
}

/// Whether an HTTP status code is transient and retryable.
fn is_retryable(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

/// Exponential backoff delay for a 1-indexed attempt.
fn backoff_delay(attempt: u32) -> Duration {
    BASE_DELAY * 2u32.saturating_pow(attempt.saturating_sub(1))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_file_specs ---------------------------------------------------

    #[test]
    fn parse_single_path_no_line() {
        let specs = parse_file_specs("src/main.rs");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(specs[0].line, None);
    }

    #[test]
    fn parse_single_path_with_line() {
        let specs = parse_file_specs("src/main.rs:42");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(specs[0].line, Some(42));
    }

    #[test]
    fn parse_multiple_specs_comma_separated() {
        let specs = parse_file_specs("a.rs:1, b.rs:20, c.rs");
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0], FileSpec { path: "a.rs".into(), line: Some(1) });
        assert_eq!(specs[1], FileSpec { path: "b.rs".into(), line: Some(20) });
        assert_eq!(specs[2], FileSpec { path: "c.rs".into(), line: None });
    }

    #[test]
    fn parse_multiple_specs_whitespace_separated() {
        let specs = parse_file_specs("a.rs:1 b.rs");
        assert_eq!(specs.len(), 2);
    }

    #[test]
    fn parse_empty_string() {
        assert!(parse_file_specs("").is_empty());
        assert!(parse_file_specs("   ").is_empty());
    }

    #[test]
    fn parse_windows_path_with_drive() {
        let specs = parse_file_specs("C:\\Users\\foo\\bar.rs:10");
        assert_eq!(specs.len(), 1);
        // The last colon-digits should be parsed as the line.
        assert_eq!(specs[0].line, Some(10));
    }

    // -- validate_no_traversal ----------------------------------------------

    #[test]
    fn traversal_blocked() {
        let result = validate_no_traversal(Path::new("../../../etc/passwd"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FilesApiError::PathTraversal(_)));
    }

    #[test]
    fn normal_path_allowed() {
        assert!(validate_no_traversal(Path::new("uploads/file.txt")).is_ok());
    }

    #[test]
    fn dot_only_path_allowed() {
        assert!(validate_no_traversal(Path::new("./uploads/file.txt")).is_ok());
    }

    #[test]
    fn absolute_path_blocked() {
        let result = validate_no_traversal(Path::new("/etc/passwd"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FilesApiError::PathTraversal(_)));
    }

    #[test]
    fn validate_within_dir_blocks_escape() {
        let dest = Path::new("/tmp/downloads");
        let escaped = Path::new("/etc/cron.d/backdoor");
        assert!(validate_within_dir(dest, escaped).is_err());
    }

    #[test]
    fn validate_within_dir_allows_child() {
        let dest = Path::new("/tmp/downloads");
        let child = Path::new("/tmp/downloads/file.txt");
        assert!(validate_within_dir(dest, child).is_ok());
    }

    // -- is_retryable -------------------------------------------------------

    #[test]
    fn retryable_statuses() {
        assert!(is_retryable(429));
        assert!(is_retryable(500));
        assert!(is_retryable(502));
        assert!(is_retryable(503));
        assert!(!is_retryable(400));
        assert!(!is_retryable(404));
        assert!(!is_retryable(200));
    }

    // -- backoff_delay ------------------------------------------------------

    #[test]
    fn backoff_delays() {
        assert_eq!(backoff_delay(1), Duration::from_secs(1));
        assert_eq!(backoff_delay(2), Duration::from_secs(2));
        assert_eq!(backoff_delay(3), Duration::from_secs(4));
    }

    // -- FileSpec equality --------------------------------------------------

    #[test]
    fn file_spec_equality() {
        let a = FileSpec { path: "foo.rs".into(), line: Some(1) };
        let b = FileSpec { path: "foo.rs".into(), line: Some(1) };
        assert_eq!(a, b);
    }

    // -- FilesApiError display ----------------------------------------------

    #[test]
    fn error_display() {
        let err = FilesApiError::FileTooLarge(100_000_000);
        assert!(err.to_string().contains("52428800"));
    }
}
