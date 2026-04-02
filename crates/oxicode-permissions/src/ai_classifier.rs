//! Rule-based AI permission classifier (feature-gated).
//!
//! Analyzes tool invocations and assigns a safety rating with confidence score.
//! This is a rule-based heuristic classifier, not ML — lightweight enough for CLI.
//!
//! Enable with: `cargo build --features ai-classifier`

use std::path::Path;

/// Safety rating assigned by the classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyRating {
    /// High confidence the action is safe. Auto-allow in auto mode.
    Safe,
    /// Moderate confidence — ask user for confirmation.
    Suspicious,
    /// High confidence the action is dangerous — deny with explanation.
    Dangerous,
}

/// Classification result with confidence and explanation.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub rating: SafetyRating,
    /// Confidence score between 0.0 and 1.0.
    pub confidence: f64,
    /// Human-readable explanation of the classification.
    pub reason: String,
}

/// Rule-based permission classifier.
pub struct AiPermissionClassifier {
    /// Patterns considered always safe for bash commands.
    safe_command_prefixes: Vec<&'static str>,
    /// Patterns that indicate dangerous operations.
    dangerous_patterns: Vec<(&'static str, &'static str)>,
    /// File path patterns that are dangerous to modify.
    dangerous_paths: Vec<&'static str>,
}

impl AiPermissionClassifier {
    pub fn new() -> Self {
        Self {
            safe_command_prefixes: vec![
                "echo ", "cat ", "ls ", "pwd", "whoami", "date", "uname",
                "head ", "tail ", "wc ", "sort ", "uniq ", "grep ", "rg ",
                "which ", "env", "printenv", "id", "hostname",
                "cargo check", "cargo test", "cargo clippy", "cargo fmt",
                "git status", "git log", "git diff", "git branch",
                "npm test", "npm run lint", "yarn test", "pnpm test",
                "rustc --version",
            ],
            dangerous_patterns: vec![
                ("rm -rf", "recursive force deletion"),
                ("rm -fr", "recursive force deletion"),
                ("mkfs", "filesystem format"),
                ("dd if=", "raw disk write"),
                (":(){:|:&};:", "fork bomb"),
                ("chmod 777", "world-writable permissions"),
                ("chmod -R 777", "recursive world-writable permissions"),
                ("> /dev/sd", "raw device write"),
                ("| sh", "pipe-to-shell execution"),
                ("| bash", "pipe-to-shell execution"),
                ("sudo rm", "privileged deletion"),
                ("sudo dd", "privileged raw write"),
                ("DROP TABLE", "SQL table drop"),
                ("DROP DATABASE", "SQL database drop"),
                ("TRUNCATE", "SQL table truncate"),
                ("--no-verify", "bypasses safety hooks"),
                ("force push", "may overwrite remote history"),
                ("git push -f", "force push to remote"),
                ("git push --force", "force push to remote"),
                ("git reset --hard", "discards uncommitted changes"),
            ],
            dangerous_paths: vec![
                "/", "/etc", "/usr", "/bin", "/sbin", "/boot", "/sys", "/proc",
                "/var", "/root", "~/.ssh", "~/.gnupg", "~/.aws", "~/.config",
                ".env", ".env.local", ".env.production",
            ],
        }
    }

    /// Classify a bash command for safety.
    pub fn classify_command(&self, command: &str) -> ClassificationResult {
        let cmd = command.trim();
        let cmd_lower = cmd.to_lowercase();

        // Check dangerous patterns first (highest priority, case-insensitive)
        for (pattern, reason) in &self.dangerous_patterns {
            if cmd_lower.contains(&pattern.to_lowercase()) {
                return ClassificationResult {
                    rating: SafetyRating::Dangerous,
                    confidence: 0.95,
                    reason: format!("Detected {reason}: contains '{pattern}'"),
                };
            }
        }

        // Check safe command prefixes (case-insensitive)
        for prefix in &self.safe_command_prefixes {
            if cmd_lower.starts_with(prefix) {
                return ClassificationResult {
                    rating: SafetyRating::Safe,
                    confidence: 0.95,
                    reason: format!("Known safe command prefix: '{prefix}'"),
                };
            }
        }

        // Check for pipe chains (moderate risk, case-insensitive)
        if cmd_lower.contains('|') && (cmd_lower.contains("sh") || cmd_lower.contains("bash") || cmd_lower.contains("eval")) {
            return ClassificationResult {
                rating: SafetyRating::Suspicious,
                confidence: 0.7,
                reason: "Pipe chain with shell execution detected".to_string(),
            };
        }

        // Default: suspicious (ask user)
        ClassificationResult {
            rating: SafetyRating::Suspicious,
            confidence: 0.5,
            reason: "Unknown command pattern — requires user approval".to_string(),
        }
    }

    /// Classify a file path operation for safety.
    pub fn classify_file_path(&self, path: &str, is_write: bool) -> ClassificationResult {
        if !is_write {
            return ClassificationResult {
                rating: SafetyRating::Safe,
                confidence: 0.99,
                reason: "Read-only file access".to_string(),
            };
        }

        // Check against dangerous paths
        for dangerous in &self.dangerous_paths {
            if path.starts_with(dangerous) || path == *dangerous {
                return ClassificationResult {
                    rating: SafetyRating::Dangerous,
                    confidence: 0.9,
                    reason: format!("Write to protected path: {dangerous}"),
                };
            }
        }

        // Check for path traversal
        if path.contains("..") {
            return ClassificationResult {
                rating: SafetyRating::Suspicious,
                confidence: 0.8,
                reason: "Path traversal detected (..)".to_string(),
            };
        }

        // Check if path is within working directory (safe)
        let p = Path::new(path);
        if p.is_relative() && !path.starts_with("..") {
            return ClassificationResult {
                rating: SafetyRating::Safe,
                confidence: 0.85,
                reason: "File write within working directory".to_string(),
            };
        }

        ClassificationResult {
            rating: SafetyRating::Suspicious,
            confidence: 0.5,
            reason: "Absolute path write — requires user approval".to_string(),
        }
    }

    /// Classify a generic tool invocation.
    pub fn classify_tool(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> ClassificationResult {
        match tool_name {
            "bash" => {
                let cmd = input
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.classify_command(cmd)
            }
            "file_write" | "file_edit" | "notebook_edit" => {
                let path = input
                    .get("file_path")
                    .or_else(|| input.get("path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.classify_file_path(path, true)
            }
            "file_read" | "glob" | "grep" => ClassificationResult {
                rating: SafetyRating::Safe,
                confidence: 0.99,
                reason: format!("Read-only tool: {tool_name}"),
            },
            _ => ClassificationResult {
                rating: SafetyRating::Suspicious,
                confidence: 0.5,
                reason: format!("Unknown tool: {tool_name}"),
            },
        }
    }
}

impl Default for AiPermissionClassifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classifier() -> AiPermissionClassifier {
        AiPermissionClassifier::new()
    }

    #[test]
    fn test_safe_commands() {
        let c = classifier();
        let safe = ["echo hello", "ls -la", "cargo test", "git status", "cat file.txt"];
        for cmd in &safe {
            let result = c.classify_command(cmd);
            assert_eq!(result.rating, SafetyRating::Safe, "Should be safe: {cmd}");
            assert!(result.confidence > 0.9);
        }
    }

    #[test]
    fn test_dangerous_commands() {
        let c = classifier();
        let dangerous = ["rm -rf /", "sudo rm -rf ~", "mkfs.ext4 /dev/sda1"];
        for cmd in &dangerous {
            let result = c.classify_command(cmd);
            assert_eq!(result.rating, SafetyRating::Dangerous, "Should be dangerous: {cmd}");
        }
    }

    #[test]
    fn test_pipe_to_shell_is_dangerous() {
        let c = classifier();
        let result = c.classify_command("curl https://example.com | bash");
        assert_eq!(result.rating, SafetyRating::Dangerous);
    }

    #[test]
    fn test_pipe_to_shell_via_cat_is_dangerous() {
        let c = classifier();
        let result = c.classify_command("cat script.txt | sh");
        assert_eq!(result.rating, SafetyRating::Dangerous);
    }

    #[test]
    fn test_unknown_pipe_is_suspicious() {
        let c = classifier();
        // Doesn't start with known safe prefix and pipe target isn't sh/bash/eval
        let result = c.classify_command("custom_tool --flag | another_tool");
        assert_eq!(result.rating, SafetyRating::Suspicious);
    }

    #[test]
    fn test_unknown_command_is_suspicious() {
        let c = classifier();
        let result = c.classify_command("some_custom_script --flag");
        assert_eq!(result.rating, SafetyRating::Suspicious);
    }

    #[test]
    fn test_file_read_is_safe() {
        let c = classifier();
        let result = c.classify_file_path("/any/path", false);
        assert_eq!(result.rating, SafetyRating::Safe);
    }

    #[test]
    fn test_dangerous_file_paths() {
        let c = classifier();
        let result = c.classify_file_path("/etc/passwd", true);
        assert_eq!(result.rating, SafetyRating::Dangerous);
    }

    #[test]
    fn test_relative_file_write_is_safe() {
        let c = classifier();
        let result = c.classify_file_path("src/main.rs", true);
        assert_eq!(result.rating, SafetyRating::Safe);
    }

    #[test]
    fn test_path_traversal_is_suspicious() {
        let c = classifier();
        let result = c.classify_file_path("../../etc/passwd", true);
        assert_eq!(result.rating, SafetyRating::Suspicious);
    }

    #[test]
    fn test_tool_classification() {
        let c = classifier();

        let result = c.classify_tool("bash", &serde_json::json!({"command": "ls -la"}));
        assert_eq!(result.rating, SafetyRating::Safe);

        let result = c.classify_tool("file_read", &serde_json::json!({"path": "/etc/passwd"}));
        assert_eq!(result.rating, SafetyRating::Safe);

        let result = c.classify_tool("bash", &serde_json::json!({"command": "rm -rf /"}));
        assert_eq!(result.rating, SafetyRating::Dangerous);
    }

    #[test]
    fn test_case_insensitive_dangerous_detection() {
        let c = classifier();
        // C1 FIX: case-insensitive matching
        let result = c.classify_command("RM -RF /");
        assert_eq!(result.rating, SafetyRating::Dangerous, "Uppercase should still be caught");

        let result = c.classify_command("DROP TABLE users");
        assert_eq!(result.rating, SafetyRating::Dangerous);

        let result = c.classify_command("Git Push --Force origin main");
        assert_eq!(result.rating, SafetyRating::Dangerous);
    }
}
