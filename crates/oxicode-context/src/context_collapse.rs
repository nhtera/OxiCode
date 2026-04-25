use std::path::Path;

use oxicode_common::{Message, OxiResult};

/// Number of recent messages preserved during a full context collapse.
const KEEP_LAST_MESSAGES: usize = 3;

/// Maximum chars from CLAUDE.md included in the projection.
const CLAUDE_MD_MAX_CHARS: usize = 500;

/// Layer-5 defense: last-resort context rebuild from working directory state.
///
/// Discards all but the 3 most-recent messages and reconstructs a minimal
/// "projection" message from the current filesystem state.
#[derive(Debug, Clone, Default)]
pub struct ContextCollapse;

impl ContextCollapse {
    pub fn new() -> Self {
        Self
    }

    /// Collapse context: keep last 3 messages + prepend a fresh projection.
    ///
    /// Returns `[projection_message, ...last_3_messages]`.
    pub fn collapse(working_dir: &Path, recent_messages: &[Message]) -> OxiResult<Vec<Message>> {
        tracing::warn!(
            dir = %working_dir.display(),
            total_messages = recent_messages.len(),
            "L5: context collapse — discarding all but last {KEEP_LAST_MESSAGES} messages"
        );

        let last_3: Vec<Message> = recent_messages
            .iter()
            .rev()
            .take(KEEP_LAST_MESSAGES)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev() // restore chronological order
            .collect();

        let projection_text = format!(
            "{}\n\
             [Context collapsed. Previous conversation history was too large. \
             Only recent messages preserved.]\n",
            Self::build_projection(working_dir)
        );

        let projection_msg = Message::user(projection_text);

        let mut result = Vec::with_capacity(1 + last_3.len());
        result.push(projection_msg);
        result.extend(last_3);

        tracing::info!(
            output_messages = result.len(),
            "L5: context collapse complete"
        );

        Ok(result)
    }

    /// Build a directory listing and optional CLAUDE.md snippet for `working_dir`.
    pub fn build_projection(working_dir: &Path) -> String {
        let mut parts = Vec::new();

        // Top-level directory listing.
        parts.push(format!("Current directory: {}", working_dir.display()));

        match std::fs::read_dir(working_dir) {
            Ok(entries) => {
                let mut names: Vec<String> = entries
                    .filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                parts.push(format!("Files: {}", names.join(", ")));
            }
            Err(e) => {
                tracing::warn!(
                    "L5: could not read directory {}: {e}",
                    working_dir.display()
                );
                parts.push("Files: (unreadable)".to_string());
            }
        }

        // Optional CLAUDE.md snippet.
        let claude_md = working_dir.join(".claude").join("CLAUDE.md");
        if claude_md.exists() {
            match std::fs::read_to_string(&claude_md) {
                Ok(content) => {
                    let truncated = truncate_chars(&content, CLAUDE_MD_MAX_CHARS);
                    parts.push(format!("CLAUDE.md (excerpt):\n{truncated}"));
                }
                Err(e) => {
                    tracing::warn!("L5: could not read CLAUDE.md: {e}");
                }
            }
        }

        parts.join("\n")
    }
}

/// Truncate `s` to at most `max_chars` UTF-8 characters.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn make_messages(n: usize) -> Vec<Message> {
        (0..n)
            .map(|i| Message::user(format!("message {i}")))
            .collect()
    }

    #[test]
    fn collapse_keeps_last_3_messages() {
        let dir = TempDir::new().unwrap();
        let messages = make_messages(10);
        let original_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();

        let result = ContextCollapse::collapse(dir.path(), &messages).unwrap();

        // First message is the projection.
        assert!(result[0].text().contains("Context collapsed"));

        // Remaining 3 should match the last 3 original messages (in order).
        let result_ids: Vec<&str> = result[1..].iter().map(|m| m.id.as_str()).collect();
        let expected_ids: Vec<&str> = original_ids[7..].iter().map(String::as_str).collect();
        assert_eq!(result_ids, expected_ids);
    }

    #[test]
    fn collapse_with_fewer_than_3_messages() {
        let dir = TempDir::new().unwrap();
        let messages = make_messages(2);

        let result = ContextCollapse::collapse(dir.path(), &messages).unwrap();

        // projection + both messages
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn build_projection_includes_directory_path() {
        let dir = TempDir::new().unwrap();
        // Create a test file so the listing is non-empty.
        std::fs::File::create(dir.path().join("test.txt")).unwrap();

        let projection = ContextCollapse::build_projection(dir.path());
        assert!(projection.contains("Current directory:"));
        assert!(projection.contains("test.txt"));
    }

    #[test]
    fn build_projection_includes_claude_md_when_present() {
        let dir = TempDir::new().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let mut f = std::fs::File::create(claude_dir.join("CLAUDE.md")).unwrap();
        writeln!(f, "# Project instructions").unwrap();

        let projection = ContextCollapse::build_projection(dir.path());
        assert!(projection.contains("CLAUDE.md"));
        assert!(projection.contains("Project instructions"));
    }

    #[test]
    fn truncate_chars_limits_length() {
        let long = "a".repeat(1000);
        let result = truncate_chars(&long, 500);
        assert!(result.len() <= 503); // 500 chars + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_chars_short_string_unchanged() {
        let s = "short";
        assert_eq!(truncate_chars(s, 100), s);
    }
}
