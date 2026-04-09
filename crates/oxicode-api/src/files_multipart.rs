//! Multipart form builder for file uploads to LLM provider APIs.
//!
//! Constructs a `reqwest::multipart::Form` from a list of local file paths,
//! auto-detecting MIME types by file extension. Validates that each file
//! exists and is readable before the form is submitted.

use std::path::{Path, PathBuf};

/// A single file part to be included in a multipart upload.
struct FilePart {
    /// The form field name (e.g. `"file"`).
    name: String,
    /// Absolute path to the file on disk.
    path: PathBuf,
    /// MIME content-type string (e.g. `"text/x-rust"`).
    content_type: String,
}

/// Incrementally builds a multipart form containing one or more file parts.
///
/// Call [`add_file`](MultipartBuilder::add_file) for each file, then call
/// [`build`](MultipartBuilder::build) to produce a `reqwest::multipart::Form`
/// ready for submission.
#[derive(Default)]
pub struct MultipartBuilder {
    parts: Vec<FilePart>,
}

impl MultipartBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a file at `path` under the form field `name`.
    ///
    /// Returns `Err` if the path does not exist or is not a regular file.
    pub fn add_file(&mut self, name: &str, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Err(format!("File not found: {}", path.display()));
        }
        if !path.is_file() {
            return Err(format!("Path is not a regular file: {}", path.display()));
        }

        let content_type = Self::detect_content_type(path).to_string();

        tracing::debug!(
            name = name,
            path = %path.display(),
            content_type = %content_type,
            "MultipartBuilder: added file part"
        );

        self.parts.push(FilePart {
            name: name.to_string(),
            path: path.to_path_buf(),
            content_type,
        });

        Ok(())
    }

    /// Detect the MIME content-type for `path` based on its file extension.
    ///
    /// Falls back to `application/octet-stream` for unknown extensions.
    pub fn detect_content_type(path: &Path) -> &'static str {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase()
            .as_str()
        {
            "rs" => "text/x-rust",
            "ts" | "tsx" => "text/typescript",
            "js" | "jsx" | "mjs" | "cjs" => "text/javascript",
            "py" => "text/x-python",
            "json" => "application/json",
            "toml" => "application/toml",
            "yaml" | "yml" => "application/yaml",
            "md" | "markdown" => "text/markdown",
            "txt" => "text/plain",
            "html" | "htm" => "text/html",
            "css" => "text/css",
            "sh" | "bash" => "application/x-sh",
            "go" => "text/x-go",
            "java" => "text/x-java-source",
            "c" | "h" => "text/x-c",
            "cpp" | "cc" | "cxx" | "hpp" => "text/x-c++",
            "rb" => "text/x-ruby",
            "kt" | "kts" => "text/x-kotlin",
            "swift" => "text/x-swift",
            "xml" => "application/xml",
            "csv" => "text/csv",
            "pdf" => "application/pdf",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "zip" => "application/zip",
            "gz" => "application/gzip",
            _ => "application/octet-stream",
        }
    }

    /// Total uncompressed size of all registered files in bytes.
    ///
    /// Files that cannot be stat'd contribute 0 bytes.
    pub fn total_size(&self) -> u64 {
        self.parts
            .iter()
            .filter_map(|p| std::fs::metadata(&p.path).ok())
            .map(|m| m.len())
            .sum()
    }

    /// Consume the builder and produce a `reqwest::multipart::Form`.
    ///
    /// Each file is read from disk and attached as a named part with the
    /// detected content-type. Returns `Err` if any file cannot be read.
    pub fn build(self) -> Result<reqwest::multipart::Form, String> {
        let mut form = reqwest::multipart::Form::new();

        for part in self.parts {
            let bytes = std::fs::read(&part.path)
                .map_err(|e| format!("Failed to read {}: {e}", part.path.display()))?;

            let file_name = part
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();

            let multipart_part = reqwest::multipart::Part::bytes(bytes)
                .file_name(file_name)
                .mime_str(&part.content_type)
                .map_err(|e| format!("Invalid MIME type '{}': {e}", part.content_type))?;

            form = form.part(part.name, multipart_part);
        }

        Ok(form)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(ext: &str, content: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(format!("test.{ext}"));
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(content).expect("write");
        (dir, path)
    }

    #[test]
    fn detect_known_extensions() {
        assert_eq!(
            MultipartBuilder::detect_content_type(Path::new("foo.rs")),
            "text/x-rust"
        );
        assert_eq!(
            MultipartBuilder::detect_content_type(Path::new("bar.ts")),
            "text/typescript"
        );
        assert_eq!(
            MultipartBuilder::detect_content_type(Path::new("baz.py")),
            "text/x-python"
        );
        assert_eq!(
            MultipartBuilder::detect_content_type(Path::new("q.json")),
            "application/json"
        );
    }

    #[test]
    fn detect_unknown_extension_fallback() {
        assert_eq!(
            MultipartBuilder::detect_content_type(Path::new("data.xyzzy")),
            "application/octet-stream"
        );
    }

    #[test]
    fn add_nonexistent_file_returns_err() {
        let mut builder = MultipartBuilder::new();
        let result = builder.add_file("file", Path::new("/nonexistent/path/file.rs"));
        assert!(result.is_err());
    }

    #[test]
    fn total_size_reflects_file_sizes() {
        let (_dir, path) = temp_file("rs", b"fn main() {}");
        let mut builder = MultipartBuilder::new();
        builder.add_file("file", &path).unwrap();
        assert_eq!(builder.total_size(), b"fn main() {}".len() as u64);
    }
}
