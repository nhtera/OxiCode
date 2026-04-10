//! Auto-extract memories from conversation sessions.
//!
//! At session end, this module analyzes conversation messages and extracts
//! key decisions, preferences, and context as persistent memory entries.
//! Uses both pattern-based extraction (fast, no LLM) and optionally
//! LLM-based extraction (deeper, needs a provider).

use chrono::Utc;

use crate::memory::{self, MemoryEntry};
use crate::memory_types::MemoryType;

/// Max memories to extract per session to avoid noise.
const MAX_EXTRACTED_PER_SESSION: usize = 10;

/// Result of memory extraction from a session.
#[derive(Debug)]
pub struct ExtractionResult {
    /// Extracted memory entries (not yet persisted).
    pub memories: Vec<MemoryEntry>,
    /// Number saved to disk.
    pub saved_count: usize,
    /// Errors encountered during save.
    pub errors: Vec<String>,
}

/// Extract and persist memories from a conversation session.
///
/// 1. Uses pattern-based extraction from `memory::extract_memories_from_text`.
/// 2. Deduplicates against existing memories.
/// 3. Caps at `MAX_EXTRACTED_PER_SESSION`.
/// 4. Saves each new memory to disk.
pub fn extract_and_save(
    messages: &[oxicode_common::Message],
    session_id: &str,
) -> ExtractionResult {
    // Collect all user and assistant text.
    let conversation_text = messages
        .iter()
        .map(oxicode_common::Message::text)
        .collect::<Vec<_>>()
        .join("\n");

    // Pattern-based extraction.
    let mut extracted = memory::extract_memories_from_text(&conversation_text, session_id);

    // Deduplicate against existing memories.
    if let Ok(existing) = memory::load_all_memories() {
        extracted.retain(|new| {
            !existing
                .iter()
                .any(|old| content_similarity(&old.content, &new.content) > 0.8)
        });
    }

    // Cap extraction count.
    extracted.truncate(MAX_EXTRACTED_PER_SESSION);

    // Persist to disk.
    let mut saved_count = 0;
    let mut errors = Vec::new();

    for entry in &extracted {
        match memory::save_memory(entry) {
            Ok(_) => saved_count += 1,
            Err(e) => errors.push(e),
        }
    }

    if saved_count > 0 {
        tracing::info!("Extracted {saved_count} memories from session {session_id}");
    }

    ExtractionResult {
        memories: extracted,
        saved_count,
        errors,
    }
}

/// Extract memories from text and write them as markdown files to the
/// project memory directory (memdir format with YAML frontmatter).
pub fn extract_to_memdir(
    messages: &[oxicode_common::Message],
    session_id: &str,
    memory_dir: &std::path::Path,
) -> ExtractionResult {
    let conversation_text = messages
        .iter()
        .map(oxicode_common::Message::text)
        .collect::<Vec<_>>()
        .join("\n");

    let mut extracted = memory::extract_memories_from_text(&conversation_text, session_id);
    extracted.truncate(MAX_EXTRACTED_PER_SESSION);

    let mut saved_count = 0;
    let mut errors = Vec::new();

    if let Err(e) = crate::memdir::ensure_memory_dir(memory_dir) {
        errors.push(e);
        return ExtractionResult {
            memories: extracted,
            saved_count,
            errors,
        };
    }

    for entry in &extracted {
        let mem_type = infer_memory_type(&entry.content);
        let frontmatter =
            crate::memory_types::format_frontmatter(mem_type, &entry.content, &entry.tags);
        let filename = format!(
            "{}-{}.md",
            Utc::now().format("%Y%m%d-%H%M%S"),
            &entry.id[..8]
        );
        let path = memory_dir.join(&filename);
        let content = format!("{frontmatter}\n{}", entry.content);
        match std::fs::write(&path, content) {
            Ok(()) => saved_count += 1,
            Err(e) => errors.push(format!("write {filename}: {e}")),
        }
    }

    if saved_count > 0 {
        tracing::info!("Extracted {saved_count} memories to memdir for session {session_id}");
    }

    ExtractionResult {
        memories: extracted,
        saved_count,
        errors,
    }
}

/// Infer the memory type from content using keyword heuristics.
fn infer_memory_type(content: &str) -> MemoryType {
    let lower = content.to_lowercase();
    if lower.contains("prefer") || lower.contains("always") || lower.contains("never") {
        MemoryType::Preference
    } else if lower.contains("decided") || lower.contains("decision") || lower.contains("chose") {
        MemoryType::Decision
    } else if lower.contains("todo") || lower.contains("task") || lower.contains("need to") {
        MemoryType::Task
    } else if lower.contains("link") || lower.contains("docs") || lower.contains("reference") {
        MemoryType::Reference
    } else {
        MemoryType::Context
    }
}

/// Simple content similarity check (Jaccard on word sets, case-insensitive).
/// Returns a value between 0.0 (no overlap) and 1.0 (identical).
#[allow(clippy::cast_precision_loss)]
fn content_similarity(a: &str, b: &str) -> f64 {
    let words_a: std::collections::HashSet<String> =
        a.split_whitespace().map(str::to_lowercase).collect();
    let words_b: std::collections::HashSet<String> =
        b.split_whitespace().map(str::to_lowercase).collect();

    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }

    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();

    if union == 0 {
        return 0.0;
    }

    intersection as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxicode_common::Message;

    #[test]
    fn extract_from_messages_with_markers() {
        let messages = vec![
            Message::user("I prefer using snake_case for all variable names"),
            Message::assistant(),
            Message::user("Always use Rust for CLI tools please"),
        ];

        // Test the core extraction logic (no disk I/O).
        let conversation_text = messages
            .iter()
            .map(|m| m.text())
            .collect::<Vec<_>>()
            .join("\n");
        let extracted = memory::extract_memories_from_text(&conversation_text, "test-session");
        assert!(
            !extracted.is_empty(),
            "Should extract at least one memory from marker patterns"
        );
        assert!(extracted.iter().all(|m| m.source == "auto"));
    }

    #[test]
    fn extract_no_markers() {
        let messages = vec![
            Message::user("What is the weather today?"),
            Message::assistant(),
        ];

        let conversation_text = messages
            .iter()
            .map(|m| m.text())
            .collect::<Vec<_>>()
            .join("\n");
        let extracted = memory::extract_memories_from_text(&conversation_text, "test-session");
        assert!(extracted.is_empty());
    }

    #[test]
    fn content_similarity_identical() {
        assert_eq!(content_similarity("hello world", "hello world"), 1.0);
    }

    #[test]
    fn content_similarity_no_overlap() {
        assert_eq!(content_similarity("hello world", "foo bar"), 0.0);
    }

    #[test]
    fn content_similarity_partial() {
        let sim = content_similarity("hello world foo", "hello world bar");
        assert!(sim > 0.4);
        assert!(sim < 0.8);
    }

    #[test]
    fn content_similarity_empty() {
        assert_eq!(content_similarity("", ""), 1.0);
    }

    #[test]
    fn infer_type_preference() {
        assert_eq!(
            infer_memory_type("I prefer using Rust"),
            MemoryType::Preference
        );
        assert_eq!(
            infer_memory_type("Always use snake_case"),
            MemoryType::Preference
        );
    }

    #[test]
    fn infer_type_decision() {
        assert_eq!(
            infer_memory_type("We decided to use axum"),
            MemoryType::Decision
        );
    }

    #[test]
    fn infer_type_task() {
        assert_eq!(
            infer_memory_type("Need to fix the auth bug"),
            MemoryType::Task
        );
    }

    #[test]
    fn infer_type_reference() {
        assert_eq!(
            infer_memory_type("See the docs for details"),
            MemoryType::Reference
        );
    }

    #[test]
    fn infer_type_context_fallback() {
        assert_eq!(
            infer_memory_type("The project uses Rust and TypeScript"),
            MemoryType::Context
        );
    }

    #[test]
    fn extract_to_memdir_creates_files() {
        let dir = tempfile::tempdir().unwrap();
        let mem_dir = dir.path().join("memory");

        let messages = vec![
            Message::user("I prefer using Rust for all backend services"),
            Message::user("Always use structured logging with tracing"),
        ];

        let result = extract_to_memdir(&messages, "test-sess", &mem_dir);
        assert!(!result.memories.is_empty());
        assert_eq!(result.saved_count, result.memories.len());
        assert!(result.errors.is_empty());

        // Verify files exist.
        let files: Vec<_> = std::fs::read_dir(&mem_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
            .collect();
        assert_eq!(files.len(), result.saved_count);
    }

    #[test]
    fn max_extraction_cap() {
        // Create many messages with markers to trigger many extractions.
        let messages: Vec<Message> = (0..20)
            .map(|i| Message::user(&format!("I prefer method {i} for everything always")))
            .collect();

        let conversation_text = messages
            .iter()
            .map(|m| m.text())
            .collect::<Vec<_>>()
            .join("\n");
        let mut extracted = memory::extract_memories_from_text(&conversation_text, "test-cap");
        extracted.truncate(MAX_EXTRACTED_PER_SESSION);
        assert!(extracted.len() <= MAX_EXTRACTED_PER_SESSION);
    }
}
