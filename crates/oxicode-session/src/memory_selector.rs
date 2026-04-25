//! LLM-based memory relevance selection.
//!
//! Given a user query and a list of memory entries (up to 200),
//! selects the top N most relevant memories by calling a fast LLM
//! (e.g., Haiku) with a structured selection prompt. Falls back to
//! recency-based selection if the LLM call fails.

use std::time::SystemTime;

use serde::Deserialize;

use crate::memory::MemoryEntry;

/// Default number of memories to select.
const DEFAULT_MAX_SELECTED: usize = 5;

/// Result of relevance selection.
#[derive(Debug)]
pub struct SelectionResult {
    /// Selected memories in relevance order.
    pub selected: Vec<MemoryEntry>,
    /// Whether LLM was used (false = recency fallback).
    pub llm_used: bool,
}

/// Trait for making LLM calls (abstracted for testability).
///
/// Implementors call a fast model (Haiku) with a prompt and return
/// the raw text response. The selector parses the response.
#[async_trait::async_trait]
pub trait SelectionLlm: Send + Sync {
    /// Send a prompt to a fast model and return the text response.
    async fn query(&self, prompt: &str) -> Result<String, String>;
}

/// Select the top N relevant memories for a given query.
///
/// 1. Builds a manifest of memory summaries with indices.
/// 2. Asks the LLM to pick the most relevant indices.
/// 3. Parses the response and returns matching entries.
/// 4. Falls back to recency-based selection on any failure.
pub async fn select_relevant(
    llm: &dyn SelectionLlm,
    query: &str,
    memories: &[MemoryEntry],
    max: Option<usize>,
) -> SelectionResult {
    let max = max.unwrap_or(DEFAULT_MAX_SELECTED);

    if memories.is_empty() {
        return SelectionResult {
            selected: Vec::new(),
            llm_used: false,
        };
    }

    // If fewer than max, return all (no selection needed).
    if memories.len() <= max {
        return SelectionResult {
            selected: memories.to_vec(),
            llm_used: false,
        };
    }

    // Build manifest for LLM.
    let manifest = build_manifest(memories);
    let prompt = build_selection_prompt(query, &manifest, max);

    // Try LLM selection.
    match llm.query(&prompt).await {
        Ok(response) => {
            let indices = parse_selection_response(&response, memories.len());
            if indices.is_empty() {
                // LLM returned unusable response — fall back.
                tracing::warn!("Memory selector: LLM returned no valid indices, using recency");
                return fallback_recency(memories, max);
            }
            let selected: Vec<MemoryEntry> = indices
                .iter()
                .filter_map(|&i| memories.get(i).cloned())
                .collect();
            SelectionResult {
                selected,
                llm_used: true,
            }
        }
        Err(e) => {
            tracing::warn!("Memory selector LLM call failed: {e}, using recency fallback");
            fallback_recency(memories, max)
        }
    }
}

/// Build a numbered manifest of memory summaries for the LLM.
fn build_manifest(memories: &[MemoryEntry]) -> String {
    use std::fmt::Write;
    let mut manifest = String::new();
    for (i, mem) in memories.iter().enumerate() {
        let tags = if mem.tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", mem.tags.join(", "))
        };
        // Truncate content to 100 chars for the manifest.
        let content: String = mem.content.chars().take(100).collect();
        let _ = writeln!(manifest, "{i}: {content}{tags}");
    }
    manifest
}

/// Build the selection prompt with XML delimiters to prevent injection.
fn build_selection_prompt(query: &str, manifest: &str, max: usize) -> String {
    format!(
        r#"You are a memory relevance selector. Given a user's query and a numbered list of memories, select the {max} most relevant memories.

<user_query>
{query}
</user_query>

<memory_manifest>
{manifest}
</memory_manifest>

Return ONLY a JSON object with the key "selected_indices" containing an array of the {max} most relevant memory indices (0-based). Example: {{"selected_indices": [2, 0, 5]}}

Order by relevance (most relevant first). If fewer than {max} are relevant, return fewer."#
    )
}

/// Parse the LLM response to extract selected indices.
/// Validates each index is within bounds and deduplicates.
fn parse_selection_response(response: &str, total: usize) -> Vec<usize> {
    // Try parsing as JSON first.
    #[derive(Deserialize)]
    struct SelectionResponse {
        selected_indices: Vec<usize>,
    }

    let dedup = |indices: Vec<usize>| -> Vec<usize> {
        let mut seen = std::collections::HashSet::new();
        indices
            .into_iter()
            .filter(|&i| i < total && seen.insert(i))
            .collect()
    };

    // Try the full response as JSON.
    if let Ok(parsed) = serde_json::from_str::<SelectionResponse>(response) {
        return dedup(parsed.selected_indices);
    }

    // Try extracting JSON from markdown code blocks.
    let trimmed = response.trim();
    let json_str = if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            &trimmed[start..=end]
        } else {
            return Vec::new();
        }
    } else {
        return Vec::new();
    };

    if let Ok(parsed) = serde_json::from_str::<SelectionResponse>(json_str) {
        return dedup(parsed.selected_indices);
    }

    Vec::new()
}

/// Fallback: return the N most recent memories (already sorted by recency).
fn fallback_recency(memories: &[MemoryEntry], max: usize) -> SelectionResult {
    SelectionResult {
        selected: memories.iter().take(max).cloned().collect(),
        llm_used: false,
    }
}

/// Format selected memories for system prompt injection.
pub fn format_memories_for_prompt(memories: &[MemoryEntry]) -> String {
    use crate::memory_freshness::annotate_memory;
    use std::fmt::Write;

    if memories.is_empty() {
        return String::new();
    }

    let mut output = String::from("## Relevant Memories\n\n");
    for mem in memories {
        let created_at = u64::try_from(mem.created_at.timestamp())
            .ok()
            .and_then(|secs| {
                SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(secs))
            })
            .unwrap_or(SystemTime::now());

        let annotated = annotate_memory(&mem.content, created_at);
        let _ = writeln!(output, "- {annotated}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    /// Mock LLM that returns a fixed response.
    struct MockLlm {
        response: Result<String, String>,
    }

    #[async_trait::async_trait]
    impl SelectionLlm for MockLlm {
        async fn query(&self, _prompt: &str) -> Result<String, String> {
            self.response.clone()
        }
    }

    fn make_memory(content: &str, tags: &[&str]) -> MemoryEntry {
        MemoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.to_string(),
            tags: tags.iter().map(|s| (*s).to_string()).collect(),
            source: "manual".to_string(),
            session_id: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn parse_valid_json_response() {
        let response = r#"{"selected_indices": [0, 2, 4]}"#;
        let indices = parse_selection_response(response, 10);
        assert_eq!(indices, vec![0, 2, 4]);
    }

    #[test]
    fn parse_json_in_code_block() {
        let response = "Here are the results:\n```json\n{\"selected_indices\": [1, 3]}\n```";
        let indices = parse_selection_response(response, 10);
        assert_eq!(indices, vec![1, 3]);
    }

    #[test]
    fn parse_filters_out_of_bounds() {
        let response = r#"{"selected_indices": [0, 50, 2]}"#;
        let indices = parse_selection_response(response, 5);
        assert_eq!(indices, vec![0, 2]);
    }

    #[test]
    fn parse_invalid_response() {
        let response = "I don't know what to pick";
        let indices = parse_selection_response(response, 10);
        assert!(indices.is_empty());
    }

    #[tokio::test]
    async fn select_with_llm_success() {
        let memories: Vec<MemoryEntry> = (0..10)
            .map(|i| make_memory(&format!("Memory {i}"), &["test"]))
            .collect();

        let llm = MockLlm {
            response: Ok(r#"{"selected_indices": [0, 3, 7]}"#.to_string()),
        };

        let result = select_relevant(&llm, "test query", &memories, Some(3)).await;
        assert!(result.llm_used);
        assert_eq!(result.selected.len(), 3);
        assert_eq!(result.selected[0].content, "Memory 0");
        assert_eq!(result.selected[1].content, "Memory 3");
        assert_eq!(result.selected[2].content, "Memory 7");
    }

    #[tokio::test]
    async fn select_falls_back_on_llm_failure() {
        let memories: Vec<MemoryEntry> = (0..10)
            .map(|i| make_memory(&format!("Memory {i}"), &["test"]))
            .collect();

        let llm = MockLlm {
            response: Err("API error".to_string()),
        };

        let result = select_relevant(&llm, "test query", &memories, Some(3)).await;
        assert!(!result.llm_used);
        assert_eq!(result.selected.len(), 3);
        // Recency fallback returns first 3 (memories are already newest-first).
        assert_eq!(result.selected[0].content, "Memory 0");
    }

    #[tokio::test]
    async fn select_returns_all_when_fewer_than_max() {
        let memories = vec![make_memory("Only one", &[]), make_memory("Just two", &[])];

        let llm = MockLlm {
            response: Err("should not be called".to_string()),
        };

        let result = select_relevant(&llm, "test", &memories, Some(5)).await;
        assert!(!result.llm_used);
        assert_eq!(result.selected.len(), 2);
    }

    #[tokio::test]
    async fn select_empty_memories() {
        let llm = MockLlm {
            response: Err("should not be called".to_string()),
        };

        let result = select_relevant(&llm, "test", &[], Some(5)).await;
        assert!(result.selected.is_empty());
    }

    #[test]
    fn build_manifest_format() {
        let memories = vec![
            make_memory("Use Rust for CLI", &["lang"]),
            make_memory("Prefer snake_case", &["style", "code"]),
        ];
        let manifest = build_manifest(&memories);
        assert!(manifest.contains("0: Use Rust for CLI [lang]"));
        assert!(manifest.contains("1: Prefer snake_case [style, code]"));
    }

    #[test]
    fn format_memories_empty() {
        let result = format_memories_for_prompt(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn format_memories_non_empty() {
        let memories = vec![make_memory("Use Rust", &[])];
        let result = format_memories_for_prompt(&memories);
        assert!(result.contains("Relevant Memories"));
        assert!(result.contains("Use Rust"));
    }
}
