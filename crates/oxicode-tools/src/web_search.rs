use std::time::Duration;

use async_trait::async_trait;
use oxicode_common::OxiResult;
use serde::Deserialize;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Search the web using the Brave Search API.
pub struct WebSearchTool;

const SEARCH_TIMEOUT_SECS: u64 = 15;
const DEFAULT_MAX_RESULTS: usize = 5;
const BRAVE_API_URL: &str = "https://api.search.brave.com/res/v1/web/search";
const USER_AGENT: &str = "OxiCode/0.1 (+https://github.com/oxicode)";

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "Search the web and return results. Requires BRAVE_SEARCH_API_KEY environment variable."
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
                        "description": "The search query"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 5, max: 20)"
                    }
                },
                "required": ["query"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }

    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let Some(query) = input["query"].as_str() else {
            return Ok(ToolResult::error("'query' is required"));
        };
        let max_results = input["max_results"]
            .as_u64()
            .map_or(DEFAULT_MAX_RESULTS, |v| (v as usize).min(20));

        let api_key = match std::env::var("BRAVE_SEARCH_API_KEY") {
            Ok(k) if !k.is_empty() => k,
            _ => {
                return Ok(ToolResult::error(
                    "BRAVE_SEARCH_API_KEY environment variable is not set. \
                     Get a free API key at https://brave.com/search/api/",
                ));
            }
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(SEARCH_TIMEOUT_SECS))
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: format!("Failed to build HTTP client: {e}"),
            })?;

        let response = match client
            .get(BRAVE_API_URL)
            .header("X-Subscription-Token", &api_key)
            .header("Accept", "application/json")
            .query(&[("q", query), ("count", &max_results.to_string())])
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult::error(format!("Search request failed: {e}")));
            }
        };

        let status = response.status().as_u16();
        if status != 200 {
            let body = response.text().await.unwrap_or_default();
            let preview: String = body.chars().take(500).collect();
            return Ok(ToolResult::error(format!(
                "Brave API returned HTTP {status}: {preview}"
            )));
        }

        let body: BraveResponse = match response.json().await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Failed to parse search response: {e}"
                )));
            }
        };

        let results = body.web.as_ref().map_or(&[][..], |w| &w.results);

        if results.is_empty() {
            return Ok(ToolResult::success(format!(
                "No results found for: {query}"
            )));
        }

        let mut output = format!("Search results for: {query}\n");
        for (i, result) in results.iter().take(max_results).enumerate() {
            use std::fmt::Write;
            let _ = write!(
                output,
                "\n{}. {}\n   {}\n   {}",
                i + 1,
                result.title,
                result.url,
                result.description.as_deref().unwrap_or("(no description)")
            );
        }

        Ok(ToolResult::success(output))
    }
}

// Brave Search API response types (only the fields we need).

#[derive(Deserialize)]
struct BraveResponse {
    web: Option<BraveWebResults>,
}

#[derive(Deserialize)]
struct BraveWebResults {
    results: Vec<BraveResult>,
}

#[derive(Deserialize)]
struct BraveResult {
    title: String,
    url: String,
    description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_search_requires_query() {
        let tool = WebSearchTool;
        let ctx = ToolContext::default();

        let result = tool.execute(serde_json::json!({}), &ctx).await.unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("'query' is required"));
    }

    #[tokio::test]
    async fn test_search_requires_api_key() {
        // Only run this test if the env var is NOT set (avoid mutating env in tests).
        if std::env::var("BRAVE_SEARCH_API_KEY").is_ok() {
            return; // Skip — key is set, can't test missing-key path safely.
        }

        let tool = WebSearchTool;
        let ctx = ToolContext::default();

        let result = tool
            .execute(serde_json::json!({"query": "rust programming"}), &ctx)
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("BRAVE_SEARCH_API_KEY"));
    }
}
