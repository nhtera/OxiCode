//! Environment variable expansion for MCP server config values.
//!
//! Supports `${VAR}`, `$VAR`, and `${VAR:-default}` syntax.
//! Only applied to trusted config sources (files, env var), never to user input.

use std::collections::HashMap;

/// Expand environment variable references in a string.
///
/// Supported patterns:
/// - `${VAR}` — replaced with env var value, empty string if unset
/// - `$VAR` — replaced with env var value (word-boundary: letters, digits, underscore)
/// - `${VAR:-default}` — replaced with env var value, or `default` if unset/empty
///
/// Performance: single-pass scan, no regex. Completes in < 1ms for typical config strings.
pub fn expand_env(input: &str) -> String {
    expand_env_with(input, |key| std::env::var(key).ok())
}

/// Expand environment variables using a custom lookup function (for testing).
pub fn expand_env_with(input: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut i = 0;

    while i < len {
        if bytes[i] == b'$' && i + 1 < len {
            if bytes[i + 1] == b'{' {
                // ${VAR} or ${VAR:-default}
                if let Some(close) = input[i + 2..].find('}') {
                    let inner = &input[i + 2..i + 2 + close];
                    let (var_name, default_val) = if let Some(pos) = inner.find(":-") {
                        (&inner[..pos], Some(&inner[pos + 2..]))
                    } else {
                        (inner, None)
                    };

                    let value = lookup(var_name);
                    match (value, default_val) {
                        (Some(ref v), _) if !v.is_empty() => result.push_str(v),
                        (_, Some(def)) => result.push_str(def),
                        (Some(v), None) => result.push_str(&v), // empty but no default
                        (None, None) => {}                      // unset, no default
                    }
                    i = i + 2 + close + 1;
                } else {
                    // No closing brace, treat literally.
                    result.push('$');
                    i += 1;
                }
            } else if bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_' {
                // $VAR — scan word characters
                let start = i + 1;
                let mut end = start;
                while end < len && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                    end += 1;
                }
                let var_name = &input[start..end];
                if let Some(value) = lookup(var_name) {
                    result.push_str(&value);
                }
                i = end;
            } else {
                // $<non-alpha>, treat literally.
                result.push('$');
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

/// Expand all environment variables in a map of key-value pairs (e.g., server env config).
pub fn expand_env_map(map: &HashMap<String, String>) -> HashMap<String, String> {
    map.iter()
        .map(|(k, v)| (k.clone(), expand_env(v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a lookup from static pairs.
    fn mock_lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn test_no_vars() {
        assert_eq!(expand_env_with("hello world", |_| None), "hello world");
    }

    #[test]
    fn test_braced_var() {
        let lookup = mock_lookup(&[("HOME", "/Users/test")]);
        assert_eq!(
            expand_env_with("${HOME}/.oxicode", lookup),
            "/Users/test/.oxicode"
        );
    }

    #[test]
    fn test_unbraced_var() {
        let lookup = mock_lookup(&[("HOME", "/Users/test")]);
        assert_eq!(
            expand_env_with("$HOME/.oxicode", lookup),
            "/Users/test/.oxicode"
        );
    }

    #[test]
    fn test_default_value_when_unset() {
        assert_eq!(
            expand_env_with("${MISSING:-fallback}", |_| None),
            "fallback"
        );
    }

    #[test]
    fn test_default_value_when_empty() {
        assert_eq!(
            expand_env_with("${EMPTY:-fallback}", |_| Some(String::new())),
            "fallback"
        );
    }

    #[test]
    fn test_default_value_ignored_when_set() {
        let lookup = mock_lookup(&[("VAR", "real")]);
        assert_eq!(expand_env_with("${VAR:-fallback}", lookup), "real");
    }

    #[test]
    fn test_multiple_vars() {
        let lookup = mock_lookup(&[("A", "1"), ("B", "2")]);
        assert_eq!(expand_env_with("$A-$B", lookup), "1-2");
    }

    #[test]
    fn test_adjacent_braced_vars() {
        let lookup = mock_lookup(&[("X", "hello"), ("Y", "world")]);
        assert_eq!(expand_env_with("${X}${Y}", lookup), "helloworld");
    }

    #[test]
    fn test_unset_var_becomes_empty() {
        assert_eq!(expand_env_with("pre${MISSING}post", |_| None), "prepost");
    }

    #[test]
    fn test_dollar_at_end() {
        assert_eq!(expand_env_with("hello$", |_| None), "hello$");
    }

    #[test]
    fn test_dollar_non_alpha() {
        assert_eq!(expand_env_with("$1 $$ $!", |_| None), "$1 $$ $!");
    }

    #[test]
    fn test_unclosed_brace() {
        assert_eq!(expand_env_with("${UNCLOSED", |_| None), "${UNCLOSED");
    }

    #[test]
    fn test_expand_env_map() {
        std::env::set_var("_TEST_EXP_MAP", "expanded");
        let mut map = HashMap::new();
        map.insert("key".to_string(), "${_TEST_EXP_MAP}/path".to_string());
        let result = expand_env_map(&map);
        assert_eq!(result["key"], "expanded/path");
        std::env::remove_var("_TEST_EXP_MAP");
    }

    #[test]
    fn test_realistic_mcp_config() {
        let lookup = mock_lookup(&[("HOME", "/home/user"), ("NODE_PATH", "/usr/local/bin/node")]);
        assert_eq!(
            expand_env_with("${HOME}/.config/mcp/server", lookup),
            "/home/user/.config/mcp/server"
        );
    }
}
