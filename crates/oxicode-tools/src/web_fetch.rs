use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::sync::LazyLock;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Fetch a URL and return its content as text.
pub struct WebFetchTool;

const FETCH_TIMEOUT_SECS: u64 = 30;
const MAX_CONTENT_BYTES: usize = 10 * 1024 * 1024; // 10 MB
const DEFAULT_MAX_CHARS: usize = 100_000;
const USER_AGENT: &str = "OxiCode/0.1 (+https://github.com/oxicode)";

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "Fetch a URL and return its content as readable text."
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
                        "description": "The URL to fetch (must be http or https)"
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "Max characters to return (default: 100000)"
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
        let Some(url_str) = input["url"].as_str() else {
            return Ok(ToolResult::error("'url' is required"));
        };
        let max_chars = input["max_length"]
            .as_u64()
            .map_or(DEFAULT_MAX_CHARS, |v| v as usize);

        // Validate URL scheme.
        if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
            return Ok(ToolResult::error(
                "Only http:// and https:// URLs are supported",
            ));
        }

        // SSRF protection: block private/loopback/link-local IPs.
        if is_private_url(url_str) {
            return Ok(ToolResult::error(
                "Requests to private/loopback/link-local addresses are blocked",
            ));
        }

        // Custom redirect policy: re-validate each hop against SSRF.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 {
                    attempt.stop()
                } else if is_private_url(attempt.url().as_str()) {
                    attempt.error(std::io::Error::other("Redirect to private address blocked"))
                } else {
                    attempt.follow()
                }
            }))
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: format!("Failed to build HTTP client: {e}"),
            })?;

        let response = match client.get(url_str).send().await {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult::error(format!("Fetch failed: {e}")));
            }
        };

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Fast reject via Content-Length header.
        if let Some(len) = response.content_length() {
            if len > MAX_CONTENT_BYTES as u64 {
                return Ok(ToolResult::error(format!(
                    "Response too large ({len} bytes, max {MAX_CONTENT_BYTES})"
                )));
            }
        }

        // Stream body with size limit to avoid OOM.
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => return Ok(ToolResult::error(format!("Failed to read body: {e}"))),
            };
            body.extend_from_slice(&chunk);
            if body.len() > MAX_CONTENT_BYTES {
                return Ok(ToolResult::error(format!(
                    "Response too large (>{MAX_CONTENT_BYTES} bytes)"
                )));
            }
        }

        let body_len = body.len();
        let raw = String::from_utf8_lossy(&body);
        let is_html = content_type.contains("text/html") || raw.trim_start().starts_with('<');

        let text = if is_html {
            strip_html(&raw)
        } else {
            raw.into_owned()
        };

        let char_count = text.chars().count();
        let truncated = truncate_chars(&text, max_chars);
        let suffix = if char_count > max_chars {
            format!("\n\n[Truncated — {max_chars} of {char_count} chars shown]")
        } else {
            String::new()
        };

        Ok(ToolResult::success(format!(
            "HTTP {status} | {content_type} | {body_len} bytes\n\n{truncated}{suffix}",
        )))
    }
}

/// Check if a URL points to a private, loopback, or link-local address.
/// Uses the `url` crate for parsing to match reqwest's parser exactly.
fn is_private_url(raw_url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(raw_url) else {
        return true; // reject unparseable
    };

    let Some(host) = parsed.host_str() else {
        return true;
    };

    // Check raw IP literal.
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_private_ip(ip);
    }

    // Block well-known internal hostnames (not file extensions — suppress false positive).
    let lower = host.to_ascii_lowercase();
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    if lower == "localhost" || lower.ends_with(".local") || lower.ends_with(".internal") {
        return true;
    }

    // DNS resolve and check all returned IPs.
    let addr = format!("{host}:80");
    match addr.to_socket_addrs() {
        Ok(addrs) => addrs.into_iter().any(|a| is_private_ip(a.ip())),
        Err(_) => false, // DNS failure handled by reqwest later
    }
}

/// Check if an IP is private, loopback, link-local, or unspecified.
fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_unspecified()
                || v4.is_link_local() // covers 169.254.0.0/16 including AWS IMDS
                || is_private_ipv4(v4)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // link-local
                // IPv4-mapped IPv6: check the embedded v4 address
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| is_private_ip(IpAddr::V4(v4)))
        }
    }
}

/// RFC 1918 private IPv4 ranges.
fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10 || (o[0] == 172 && (16..=31).contains(&o[1])) || (o[0] == 192 && o[1] == 168)
}

// Lazy-compiled regexes for HTML stripping.
static RE_SCRIPT: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap());
static RE_STYLE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap());
static RE_NOSCRIPT: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?is)<noscript[^>]*>.*?</noscript>").unwrap());
static RE_NEWLINE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)</?(br|p|div|li|h[1-6]|tr|dt|dd)[^>]*>").unwrap());
static RE_TAGS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
static RE_SPACES: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[^\S\n]+").unwrap());
static RE_BLANK: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\n{3,}").unwrap());

/// Strip HTML tags and collapse whitespace to produce readable plain text.
fn strip_html(html: &str) -> String {
    let text = RE_SCRIPT.replace_all(html, "");
    let text = RE_STYLE.replace_all(&text, "");
    let text = RE_NOSCRIPT.replace_all(&text, "");
    let text = RE_NEWLINE.replace_all(&text, "\n");
    let text = RE_TAGS.replace_all(&text, "");

    // Decode common HTML entities.
    let text = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    let text = RE_SPACES.replace_all(&text, " ");
    let text = RE_BLANK.replace_all(&text, "\n\n");

    text.trim().to_string()
}

/// Truncate string to at most `max` characters, cutting at a char boundary.
fn truncate_chars(s: &str, max: usize) -> &str {
    let end = s.char_indices().nth(max).map_or(s.len(), |(idx, _)| idx);
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_basic() {
        let html = "<html><body><h1>Hello</h1><p>World</p></body></html>";
        let text = strip_html(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(!text.contains('<'));
    }

    #[test]
    fn test_strip_html_removes_scripts() {
        let html = "<p>Before</p><script>alert('xss')</script><p>After</p>";
        let text = strip_html(html);
        assert!(text.contains("Before"));
        assert!(text.contains("After"));
        assert!(!text.contains("alert"));
    }

    #[test]
    fn test_strip_html_entities() {
        let html = "<p>A &amp; B &lt; C</p>";
        let text = strip_html(html);
        assert!(text.contains("A & B < C"));
    }

    #[test]
    fn test_truncate_chars_ascii() {
        assert_eq!(truncate_chars("hello world", 5), "hello");
    }

    #[test]
    fn test_truncate_chars_multibyte() {
        let s = "\u{4F60}\u{597D}\u{4E16}"; // 你好世
        let truncated = truncate_chars(s, 2);
        assert_eq!(truncated, "\u{4F60}\u{597D}"); // 你好
    }

    #[test]
    fn test_is_private_url_blocks_localhost() {
        assert!(is_private_url("http://localhost/secret"));
        assert!(is_private_url("http://127.0.0.1/admin"));
        assert!(is_private_url("http://[::1]:8080/"));
    }

    #[test]
    fn test_is_private_url_blocks_rfc1918() {
        assert!(is_private_url("http://10.0.0.1/"));
        assert!(is_private_url("http://172.16.0.1/"));
        assert!(is_private_url("http://192.168.1.1/"));
    }

    #[test]
    fn test_is_private_url_blocks_aws_imds() {
        assert!(is_private_url("http://169.254.169.254/latest/meta-data/"));
    }

    #[test]
    fn test_is_private_url_allows_public() {
        assert!(!is_private_url("https://example.com/page"));
    }

    #[test]
    fn test_ssrf_bypass_fragment_blocked() {
        // Parser-differential: fragment confusion must be blocked.
        assert!(is_private_url("http://127.0.0.1#@evil.com/"));
    }

    #[test]
    fn test_ssrf_bypass_query_blocked() {
        // Parser-differential: query confusion must be blocked.
        assert!(is_private_url("http://127.0.0.1?@evil.com/"));
    }

    #[test]
    fn test_ssrf_bypass_ipv4_mapped_v6_blocked() {
        assert!(is_private_url("http://[::ffff:127.0.0.1]/"));
        assert!(is_private_url("http://[::ffff:10.0.0.1]/"));
        assert!(is_private_url("http://[::ffff:169.254.169.254]/"));
    }

    #[tokio::test]
    async fn test_url_validation() {
        let tool = WebFetchTool;
        let ctx = ToolContext::default();

        let result = tool
            .execute(serde_json::json!({"url": "file:///etc/passwd"}), &ctx)
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("http"));
    }

    #[tokio::test]
    async fn test_ssrf_blocked_in_execute() {
        let tool = WebFetchTool;
        let ctx = ToolContext::default();

        let result = tool
            .execute(
                serde_json::json!({"url": "http://169.254.169.254/latest/meta-data/"}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("private"));
    }
}
