//! Remote trigger tool: fire an HTTP request to trigger a remote agent or webhook.

use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

pub struct RemoteTriggerTool;

#[async_trait]
impl Tool for RemoteTriggerTool {
    fn name(&self) -> &str {
        "remote_trigger"
    }
    fn description(&self) -> &str {
        "Trigger a remote agent or webhook via HTTP POST."
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to trigger"
                    },
                    "payload": {
                        "type": "object",
                        "description": "JSON payload to send"
                    },
                    "headers": {
                        "type": "object",
                        "description": "Additional HTTP headers"
                    }
                },
                "required": ["url"]
            }),
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> OxiResult<ToolResult> {
        let url = match input.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return Ok(ToolResult::error("url is required")),
        };

        // SSRF protection: block private/loopback/link-local/metadata URLs.
        if let Err(reason) = validate_url_safety(url) {
            return Ok(ToolResult::error(format!("Blocked URL: {reason}")));
        }

        let payload = input.get("payload").cloned().unwrap_or(serde_json::json!({}));

        let client = reqwest::Client::new();
        let mut req = client.post(url).json(&payload);

        // Apply custom headers.
        if let Some(headers) = input.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in headers {
                if let Some(val) = v.as_str() {
                    req = req.header(k.as_str(), val);
                }
            }
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                if (200..300).contains(&status.into()) {
                    Ok(ToolResult::success(format!("HTTP {status}: {body}")))
                } else {
                    Ok(ToolResult::error(format!("HTTP {status}: {body}")))
                }
            }
            Err(e) => Ok(ToolResult::error(format!("Request failed: {e}"))),
        }
    }
}

/// Validate a URL is safe to request (no SSRF to internal services).
fn validate_url_safety(url: &str) -> Result<(), &'static str> {
    let lower = url.to_lowercase();

    // Must be HTTP(S).
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return Err("only http:// and https:// URLs allowed");
    }

    // Extract host portion.
    let host = lower
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");

    // Block localhost/loopback.
    if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]" {
        return Err("localhost/loopback addresses blocked");
    }

    // Block AWS/GCP/Azure metadata endpoints.
    if host == "169.254.169.254" || host == "metadata.google.internal" {
        return Err("cloud metadata endpoints blocked");
    }

    // Block private IP ranges (10.x, 172.16-31.x, 192.168.x).
    if let Some(first_octet) = host.split('.').next().and_then(|s| s.parse::<u8>().ok()) {
        if first_octet == 10 {
            return Err("private IP range (10.x) blocked");
        }
        if first_octet == 192 {
            if let Some(second) = host.split('.').nth(1).and_then(|s| s.parse::<u8>().ok()) {
                if second == 168 {
                    return Err("private IP range (192.168.x) blocked");
                }
            }
        }
        if first_octet == 172 {
            if let Some(second) = host.split('.').nth(1).and_then(|s| s.parse::<u8>().ok()) {
                if (16..=31).contains(&second) {
                    return Err("private IP range (172.16-31.x) blocked");
                }
            }
        }
    }

    Ok(())
}
