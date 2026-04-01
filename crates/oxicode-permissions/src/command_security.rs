use regex::Regex;

/// Checks for shell-specific attack patterns (zsh/bash exploits).
pub struct CommandSecurityChecker {
    patterns: Vec<SecurityPattern>,
}

struct SecurityPattern {
    regex: Regex,
    description: String,
}

impl CommandSecurityChecker {
    pub fn new() -> Self {
        // C2 FIX: Removed overly broad $() and backtick patterns that blocked
        // all normal shell usage. Only flag truly dangerous patterns.
        let patterns = vec![
            // Zsh-specific attacks
            ("=\\(", "Zsh equals expansion attack"),
            ("zmodload", "Zsh module loading"),
            // Path traversal
            ("\\.\\./\\.\\./\\.\\./", "Deep path traversal"),
            // Environment manipulation
            ("export\\s+LD_PRELOAD", "LD_PRELOAD injection"),
            ("export\\s+DYLD_", "macOS dylib injection"),
            // Network exfiltration
            ("nc\\s+-l", "Netcat listener"),
            ("ncat\\s+", "Ncat usage"),
        ];

        let compiled = patterns
            .into_iter()
            .filter_map(|(pattern, desc)| {
                Regex::new(pattern).ok().map(|regex| SecurityPattern {
                    regex,
                    description: desc.to_string(),
                })
            })
            .collect();

        Self { patterns: compiled }
    }

    /// Check if the tool input contains shell attack patterns.
    /// Returns the reason if an attack is detected, None if safe.
    pub fn check(&self, input: &serde_json::Value) -> Option<String> {
        let command = input["command"].as_str().unwrap_or_default();

        for pattern in &self.patterns {
            if pattern.regex.is_match(command) {
                return Some(format!("Security risk: {}", pattern.description));
            }
        }

        None
    }
}

impl Default for CommandSecurityChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_path_traversal() {
        let c = CommandSecurityChecker::new();
        let result = c.check(&serde_json::json!({"command": "cat ../../../etc/passwd"}));
        assert!(result.is_some());
    }

    #[test]
    fn test_detects_ld_preload() {
        let c = CommandSecurityChecker::new();
        let result =
            c.check(&serde_json::json!({"command": "export LD_PRELOAD=/tmp/evil.so && ./app"}));
        assert!(result.is_some());
    }

    #[test]
    fn test_safe_command() {
        let c = CommandSecurityChecker::new();
        let result = c.check(&serde_json::json!({"command": "cargo build"}));
        assert!(result.is_none());
    }

    #[test]
    fn test_detects_zsh_module_load() {
        let c = CommandSecurityChecker::new();
        let result = c.check(&serde_json::json!({"command": "zmodload zsh/net/tcp"}));
        assert!(result.is_some());
    }
}
