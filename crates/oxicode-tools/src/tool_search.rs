use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::registry::ToolRegistry;
use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// List all available tools and their schemas.
pub struct ToolSearchTool;

/// Compute Levenshtein edit distance between two strings (≤ 25 LoC).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    // dp[i][j] = edit distance between a[..i] and b[..j].
    let mut dp: Vec<Vec<usize>> = (0..=m)
        .map(|i| {
            let mut row = vec![0usize; n + 1];
            row[0] = i;
            row
        })
        .collect();
    for (j, cell) in dp[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1]
            } else {
                1 + dp[i - 1][j - 1].min(dp[i - 1][j]).min(dp[i][j - 1])
            };
        }
    }
    dp[m][n]
}

/// Cap a string to `max_chars` characters for scoring efficiency.
fn cap_str(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Search registered tools by `query` and return up to `max` ranked results.
///
/// Scoring (higher = better match):
/// - 100: exact name match
/// - 50:  query is a substring of the name
/// - 25:  query is a substring of the description or any input_schema description field
/// - 10:  Levenshtein distance ≤ 3 between query and name
///
/// All string comparisons are case-insensitive. Inputs are capped at 200 chars
/// before any Levenshtein computation to stay within the 5 ms budget.
pub fn search_tools(registry: &ToolRegistry, query: &str, max: usize) -> Vec<serde_json::Value> {
    let q = query.to_lowercase();

    let mut scored: Vec<(u32, serde_json::Value)> = registry
        .list()
        .into_iter()
        .filter_map(|schema| {
            let name_lc = schema.name.to_lowercase();
            let desc_lc = cap_str(&schema.description, 200).to_lowercase();

            let score: u32 = if q.is_empty() {
                // Empty query: return all tools with score 1.
                1
            } else if name_lc == q {
                100
            } else if name_lc.contains(q.as_str()) {
                50
            } else if desc_lc.contains(q.as_str()) || schema_desc_contains(&schema.input_schema, &q) {
                25
            } else if levenshtein(cap_str(&name_lc, 200), cap_str(&q, 200)) <= 3 {
                10
            } else {
                return None;
            };

            let entry = serde_json::json!({
                "name": schema.name,
                "description": schema.description,
                "score": score,
                "input_schema": schema.input_schema,
            });
            Some((score, entry))
        })
        .collect();

    // Sort descending by score, then alphabetically by name for determinism.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| {
        let na = a.1["name"].as_str().unwrap_or("");
        let nb = b.1["name"].as_str().unwrap_or("");
        na.cmp(nb)
    }));

    scored.into_iter().take(max).map(|(_, v)| v).collect()
}

/// Check whether any `description` field inside an `input_schema` contains `needle`.
fn schema_desc_contains(schema: &serde_json::Value, needle: &str) -> bool {
    match schema {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                if key == "description" {
                    if let Some(s) = val.as_str() {
                        if cap_str(s, 200).to_lowercase().contains(needle) {
                            return true;
                        }
                    }
                } else if schema_desc_contains(val, needle) {
                    return true;
                }
            }
            false
        }
        serde_json::Value::Array(arr) => arr.iter().any(|v| schema_desc_contains(v, needle)),
        _ => false,
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "tool_search"
    }

    fn description(&self) -> &str {
        "Search and list available tools with their descriptions and schemas."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query to filter tools by name or description"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 5)"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    /// Fallback when called outside the QueryEngine (e.g., unit tests).
    /// The engine intercepts this tool and calls `search_tools` directly.
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let query = input["query"].as_str().unwrap_or("");
        Ok(ToolResult::success(format!(
            "[TOOL_SEARCH] query={query} — use within the query engine for live registry results"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_trait::PermissionLevel as PL;
    use async_trait::async_trait;

    // ---- helpers ----

    fn make_registry() -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(FileTool));
        reg.register(Box::new(BashTool));
        reg.register(Box::new(NotebookEditTool));
        reg
    }

    struct FileTool;
    struct BashTool;
    struct NotebookEditTool;

    macro_rules! dummy_tool {
        ($s:ident, $name:expr, $desc:expr) => {
            #[async_trait]
            impl Tool for $s {
                fn name(&self) -> &str { $name }
                fn description(&self) -> &str { $desc }
                fn schema(&self) -> ToolSchema {
                    ToolSchema {
                        name: $name.into(),
                        description: $desc.into(),
                        input_schema: serde_json::json!({
                            "type": "object",
                            "properties": {
                                "path": {
                                    "type": "string",
                                    "description": "File path to manipulate"
                                }
                            }
                        }),
                    }
                }
                fn permission_level(&self) -> PL { PL::ReadOnly }
                async fn execute(&self, _: serde_json::Value, _: &ToolContext) -> OxiResult<ToolResult> {
                    Ok(ToolResult::success("ok"))
                }
            }
        };
    }

    dummy_tool!(FileTool, "file_edit", "Edit a file on disk");
    dummy_tool!(BashTool, "bash", "Execute shell commands");
    dummy_tool!(NotebookEditTool, "notebook_edit", "Edit Jupyter notebook cells");

    // ---- levenshtein unit tests ----

    #[test]
    fn levenshtein_identical() {
        assert_eq!(levenshtein("abc", "abc"), 0);
    }

    #[test]
    fn levenshtein_one_typo() {
        assert_eq!(levenshtein("bash", "bsh"), 1);
        assert_eq!(levenshtein("bash", "baah"), 1);
    }

    #[test]
    fn levenshtein_empty() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", ""), 0);
    }

    // ---- search_tools tests ----

    #[test]
    fn search_empty_query_returns_all() {
        let reg = make_registry();
        let results = search_tools(&reg, "", 10);
        assert_eq!(results.len(), 3, "empty query should return all tools");
        // All have score 1.
        for r in &results {
            assert_eq!(r["score"], 1);
        }
    }

    #[test]
    fn search_exact_name_match_ranks_first() {
        let reg = make_registry();
        let results = search_tools(&reg, "bash", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0]["name"], "bash");
        assert_eq!(results[0]["score"], 100);
    }

    #[test]
    fn search_substring_in_name() {
        let reg = make_registry();
        let results = search_tools(&reg, "edit", 5);
        // "file_edit" and "notebook_edit" contain "edit" in name → score 50.
        assert!(results.len() >= 2);
        let names: Vec<&str> = results.iter().map(|r| r["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"file_edit"));
        assert!(names.contains(&"notebook_edit"));
        // All results should have score ≥ 25.
        for r in &results {
            assert!(r["score"].as_u64().unwrap() >= 25);
        }
    }

    #[test]
    fn search_typo_levenshtein() {
        let reg = make_registry();
        // "bsh" is Levenshtein 1 from "bash"
        let results = search_tools(&reg, "bsh", 5);
        assert!(!results.is_empty());
        let names: Vec<&str> = results.iter().map(|r| r["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"bash"), "fuzzy match should find 'bash' for query 'bsh'");
    }

    #[test]
    fn search_no_match_returns_empty() {
        let reg = make_registry();
        let results = search_tools(&reg, "zzzzqqqq", 5);
        assert!(results.is_empty(), "no match should return empty list");
    }

    #[test]
    fn search_respects_max() {
        let reg = make_registry();
        let results = search_tools(&reg, "", 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_result_has_required_fields() {
        let reg = make_registry();
        let results = search_tools(&reg, "bash", 1);
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert!(r["name"].is_string());
        assert!(r["description"].is_string());
        assert!(r["score"].is_number());
        assert!(r["input_schema"].is_object());
    }

    // ---- ToolSearchTool trait tests ----

    #[test]
    fn test_name_and_description() {
        let tool = ToolSearchTool;
        assert_eq!(tool.name(), "tool_search");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_schema_has_query_required() {
        let tool = ToolSearchTool;
        let schema = tool.schema();
        assert_eq!(schema.name, "tool_search");
        let required = schema.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "query"));
    }

    #[test]
    fn test_permission_level() {
        let tool = ToolSearchTool;
        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
    }

    #[tokio::test]
    async fn test_execute_with_query() {
        let tool = ToolSearchTool;
        let input = serde_json::json!({"query": "bash"});
        let result = tool.execute(input, &ToolContext::default()).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("bash"));
    }

    #[tokio::test]
    async fn test_execute_empty_query() {
        let tool = ToolSearchTool;
        let input = serde_json::json!({});
        let result = tool.execute(input, &ToolContext::default()).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("TOOL_SEARCH"));
    }
}
