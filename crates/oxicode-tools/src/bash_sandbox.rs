use std::path::{Component, Path, PathBuf};

/// Result of validating file paths within a command.
#[derive(Debug, Clone)]
pub struct PathVerdict {
    pub path: String,
    pub operation: PathOperation,
    pub is_traversal: bool,
    pub outside_bounds: bool,
}

/// Whether a path is being read or written by the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathOperation {
    Read,
    Write,
    Unknown,
}

/// Validates file paths extracted from bash commands against working directory bounds.
///
/// Detects path traversal (../ sequences) and classifies operations as read vs write
/// using a conservative heuristic (unknown → treated as write).
///
/// Wired into `BashTool::execute()` — blocks commands with paths that escape the
/// working directory before execution begins.
pub struct PathValidator;

impl PathValidator {
    /// Validate all file-like tokens in a command against the allowed directory.
    pub fn validate(command: &str, working_dir: &Path) -> Vec<PathVerdict> {
        let tokens = extract_path_tokens(command);
        let operation = classify_command_operation(command);

        tokens
            .into_iter()
            .map(|token| {
                let is_traversal = has_traversal(&token);
                let outside_bounds = is_outside_bounds(&token, working_dir);
                PathVerdict {
                    path: token,
                    operation,
                    is_traversal,
                    outside_bounds,
                }
            })
            .collect()
    }

    /// Quick check: does the command contain any paths that escape the working dir?
    pub fn has_escape(command: &str, working_dir: &Path) -> bool {
        Self::validate(command, working_dir)
            .iter()
            .any(|v| v.is_traversal || v.outside_bounds)
    }
}

/// Extract tokens that look like file paths from a command string.
/// Heuristic: tokens starting with `/`, `./`, `../`, or `~`.
fn extract_path_tokens(command: &str) -> Vec<String> {
    // Simple shell-aware tokenizer: split on whitespace but respect single/double quotes.
    let tokens = shell_tokenize(command);
    tokens
        .into_iter()
        .filter(|t| {
            t.starts_with('/') || t.starts_with("./") || t.starts_with("../") || t.starts_with('~')
        })
        .collect()
}

/// Minimal shell tokenizer that respects quoted strings.
fn shell_tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escape_next = false;

    for ch in input.chars() {
        if escape_next {
            current.push(ch);
            escape_next = false;
            continue;
        }
        if ch == '\\' && !in_single {
            escape_next = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if ch.is_whitespace() && !in_single && !in_double {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Classify the overall command as read or write operation.
fn classify_command_operation(command: &str) -> PathOperation {
    let cmd = command.trim();
    // Extract first command word (before pipes, semicolons)
    let first_word = cmd
        .split(&['|', ';', '&'][..])
        .next()
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("");

    match first_word {
        // Read-only commands
        "cat" | "head" | "tail" | "less" | "more" | "wc" | "file" | "stat" | "ls" | "find"
        | "grep" | "rg" | "ag" | "diff" | "strings" | "hexdump" | "xxd" | "md5sum"
        | "sha256sum" | "du" | "df" | "readlink" => PathOperation::Read,
        // Write commands
        "cp" | "mv" | "rm" | "touch" | "mkdir" | "rmdir" | "chmod" | "chown" | "chgrp"
        | "install" | "truncate" | "shred" | "tee" => PathOperation::Write,
        // Editors/write-capable
        "sed" if cmd.contains("-i") => PathOperation::Write,
        // Default: unknown = conservative
        _ => {
            // Check for output redirection
            if cmd.contains(" > ") || cmd.contains(" >> ") {
                PathOperation::Write
            } else {
                PathOperation::Unknown
            }
        }
    }
}

/// Check if a path string contains traversal sequences.
fn has_traversal(path: &str) -> bool {
    let p = Path::new(path);
    p.components()
        .filter(|c| matches!(c, Component::ParentDir))
        .count()
        >= 2
}

/// Check if a path resolves outside the allowed working directory.
fn is_outside_bounds(path_str: &str, working_dir: &Path) -> bool {
    let path = Path::new(path_str);
    // Only check absolute paths or paths with traversal
    if path.is_absolute() {
        // Absolute path: check if it starts with working_dir
        let canonical_wd = normalize_path(working_dir);
        let canonical_path = normalize_path(path);
        !canonical_path.starts_with(&canonical_wd)
    } else {
        // Relative path: resolve against working_dir
        let resolved = normalize_path(&working_dir.join(path));
        let canonical_wd = normalize_path(working_dir);
        !resolved.starts_with(&canonical_wd)
    }
}

/// Normalize a path without filesystem access (resolve `.` and `..`).
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                if !components.is_empty() {
                    components.pop();
                }
            }
            Component::CurDir => {}
            other => components.push(other),
        }
    }
    components.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_cat_local() {
        let verdicts = PathValidator::validate("cat ./src/main.rs", Path::new("/project"));
        assert_eq!(verdicts.len(), 1);
        assert_eq!(verdicts[0].operation, PathOperation::Read);
        assert!(!verdicts[0].is_traversal);
    }

    #[test]
    fn detect_traversal() {
        let verdicts = PathValidator::validate("cat ../../../etc/passwd", Path::new("/project"));
        assert_eq!(verdicts.len(), 1);
        assert!(verdicts[0].is_traversal);
    }

    #[test]
    fn absolute_outside_bounds() {
        assert!(PathValidator::has_escape(
            "cat /etc/passwd",
            Path::new("/project")
        ));
    }

    #[test]
    fn absolute_inside_bounds() {
        assert!(!PathValidator::has_escape(
            "cat /project/src/lib.rs",
            Path::new("/project")
        ));
    }

    #[test]
    fn write_classified() {
        let verdicts = PathValidator::validate("rm ./old_file", Path::new("/project"));
        assert_eq!(verdicts.len(), 1);
        assert_eq!(verdicts[0].operation, PathOperation::Write);
    }

    #[test]
    fn sed_inplace_is_write() {
        let op = classify_command_operation("sed -i 's/foo/bar/' file.txt");
        assert_eq!(op, PathOperation::Write);
    }

    #[test]
    fn redirect_is_write() {
        let op = classify_command_operation("echo data > /tmp/out.txt");
        assert_eq!(op, PathOperation::Write);
    }

    #[test]
    fn no_path_tokens() {
        let verdicts = PathValidator::validate("echo hello world", Path::new("/project"));
        assert!(verdicts.is_empty());
    }

    #[test]
    fn quoted_path_extracted() {
        let tokens = extract_path_tokens("cat './src/main.rs'");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], "./src/main.rs");
    }

    #[test]
    fn tilde_path_extracted() {
        let tokens = extract_path_tokens("ls ~/Documents");
        assert_eq!(tokens.len(), 1);
        assert!(tokens[0].starts_with('~'));
    }

    #[test]
    fn normalize_removes_parent() {
        let p = normalize_path(Path::new("/a/b/../c"));
        assert_eq!(p, PathBuf::from("/a/c"));
    }

    #[test]
    fn normalize_removes_cur_dir() {
        let p = normalize_path(Path::new("/a/./b/./c"));
        assert_eq!(p, PathBuf::from("/a/b/c"));
    }
}
