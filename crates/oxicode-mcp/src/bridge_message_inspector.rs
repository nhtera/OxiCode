//! Message formatting and sanitisation helpers for MCP bridge debug views.
//!
//! Enabled only when the `bridge_debug` feature flag is active.

#[cfg(feature = "bridge_debug")]
use serde_json::Value;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Pretty-print a bridge message JSON value for debug display.
///
/// Falls back to compact JSON when pretty-printing fails.
#[cfg(feature = "bridge_debug")]
pub fn format_message(msg: &Value) -> String {
    serde_json::to_string_pretty(msg).unwrap_or_else(|_| msg.to_string())
}

/// Redact any `Authorization` header values inside a message JSON in-place.
///
/// Walks object keys recursively; replaces the value of any key whose
/// case-insensitive name is `"authorization"` with `"[REDACTED]"`.
#[cfg(feature = "bridge_debug")]
pub fn redact_auth_headers(msg: &mut Value) {
    match msg {
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if key.to_ascii_lowercase() == "authorization" {
                    *val = Value::String("[REDACTED]".to_string());
                } else {
                    redact_auth_headers(val);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_auth_headers(item);
            }
        }
        // Scalar values — nothing to redact.
        _ => {}
    }
}

/// Return the compact JSON representation of `msg`, truncated to `max_len` bytes.
///
/// Appends `"…"` when truncated so the caller knows output was cut.
#[cfg(feature = "bridge_debug")]
pub fn truncate_payload(msg: &Value, max_len: usize) -> String {
    let s = msg.to_string();
    if s.len() <= max_len {
        return s;
    }
    // Truncate at a valid UTF-8 boundary.
    let cut = s
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i < max_len)
        .last()
        .unwrap_or(0);
    format!("{}…", &s[..cut])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "bridge_debug"))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_format_message_pretty() {
        let v = json!({"id": 1, "method": "tools/list"});
        let out = format_message(&v);
        assert!(out.contains("method"));
        // Pretty output should have newlines.
        assert!(out.contains('\n'));
    }

    #[test]
    fn test_redact_auth_headers() {
        let mut msg = json!({
            "headers": {
                "Authorization": "Bearer secret-token",
                "Content-Type": "application/json"
            }
        });
        redact_auth_headers(&mut msg);
        let auth = &msg["headers"]["Authorization"];
        assert_eq!(auth.as_str(), Some("[REDACTED]"));
        // Non-auth header should be untouched.
        assert_eq!(
            msg["headers"]["Content-Type"].as_str(),
            Some("application/json")
        );
    }

    #[test]
    fn test_truncate_payload_short() {
        let v = json!({"k": "v"});
        let out = truncate_payload(&v, 1000);
        // No truncation marker expected.
        assert!(!out.contains('…'));
    }

    #[test]
    fn test_truncate_payload_long() {
        let long_val = json!({"data": "x".repeat(200)});
        let out = truncate_payload(&long_val, 50);
        assert!(out.ends_with('…'));
        // Result length should be ≤ max_len + a few bytes for the ellipsis.
        assert!(out.len() <= 60);
    }
}
