//! SSE transport for MCP: connects to a remote server via HTTP SSE.
//!
//! Sends requests via POST, receives responses via SSE stream.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use oxicode_common::{OxiError, OxiResult};

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// SSE-based MCP transport for remote servers.
pub struct SseTransport {
    client: reqwest::Client,
    base_url: String,
    next_id: AtomicU64,
}

impl SseTransport {
    /// Create a new SSE transport pointing at the given MCP server URL.
    pub fn new(base_url: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_default();

        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            next_id: AtomicU64::new(1),
        }
    }

    /// Send a JSON-RPC request via POST and parse the response.
    pub async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> OxiResult<serde_json::Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest::new(id, method, params);

        let response = self
            .client
            .post(&self.base_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| OxiError::Other(format!("MCP SSE request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(OxiError::Other(format!("MCP SSE error {status}: {body}")));
        }

        let resp: JsonRpcResponse = response
            .json()
            .await
            .map_err(|e| OxiError::Other(format!("Failed to parse MCP SSE response: {e}")))?;

        if let Some(error) = resp.error {
            return Err(OxiError::Other(format!("MCP error: {error}")));
        }

        resp.result
            .ok_or_else(|| OxiError::Other("MCP SSE response has no result".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_transport_creation() {
        let transport = SseTransport::new("http://localhost:3000/mcp");
        assert_eq!(transport.base_url, "http://localhost:3000/mcp");
    }

    #[test]
    fn test_trailing_slash_trimmed() {
        let transport = SseTransport::new("http://localhost:3000/mcp/");
        assert_eq!(transport.base_url, "http://localhost:3000/mcp");
    }
}
