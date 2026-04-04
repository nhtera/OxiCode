//! Memory type taxonomy and YAML frontmatter parsing.
//!
//! Memory files use `---` delimited YAML frontmatter for metadata,
//! followed by markdown content. Types classify memory purpose.

use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Memory file classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    /// Architectural or design decisions.
    Decision,
    /// Project context or facts.
    Context,
    /// User preferences or coding style.
    Preference,
    /// Active tasks or todos.
    Task,
    /// Reference material or links.
    Reference,
}

impl MemoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Context => "context",
            Self::Preference => "preference",
            Self::Task => "task",
            Self::Reference => "reference",
        }
    }
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parsed metadata from a memory file's YAML frontmatter.
#[derive(Debug, Clone)]
pub struct MemoryHeader {
    pub filename: String,
    pub filepath: PathBuf,
    pub memory_type: Option<MemoryType>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub mtime: SystemTime,
}

/// Frontmatter fields parsed from YAML between `---` markers.
#[derive(Debug, Deserialize)]
struct RawFrontmatter {
    #[serde(rename = "type")]
    memory_type: Option<String>,
    description: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

/// Parsed frontmatter fields.
#[derive(Debug, Clone, Default)]
pub struct FrontmatterFields {
    pub memory_type: Option<MemoryType>,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

/// Parse YAML frontmatter from a markdown file's content.
/// Returns (frontmatter_fields, body_content).
/// If no frontmatter found, returns (None, full_content).
pub fn parse_frontmatter(content: &str) -> (Option<FrontmatterFields>, &str) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (None, content);
    }

    // Find closing `---` (skip the opening one).
    let after_open = &trimmed[3..];
    let close_pos = after_open.find("\n---");
    let Some(pos) = close_pos else {
        return (None, content);
    };

    let yaml_str = &after_open[..pos];
    let body_start = 3 + pos + 4; // skip opening "---" + yaml + "\n---"
    let body = trimmed
        .get(body_start..)
        .unwrap_or("")
        .trim_start_matches('\n');

    // Parse YAML.
    let parsed: Result<RawFrontmatter, _> = serde_yaml::from_str(yaml_str);
    match parsed {
        Ok(raw) => {
            let mem_type = raw.memory_type.as_deref().and_then(parse_memory_type);
            (
                Some(FrontmatterFields {
                    memory_type: mem_type,
                    description: raw.description,
                    tags: raw.tags,
                }),
                body,
            )
        }
        Err(_) => (None, content),
    }
}

fn parse_memory_type(s: &str) -> Option<MemoryType> {
    match s.to_lowercase().as_str() {
        "decision" => Some(MemoryType::Decision),
        "context" => Some(MemoryType::Context),
        "preference" => Some(MemoryType::Preference),
        "task" => Some(MemoryType::Task),
        "reference" => Some(MemoryType::Reference),
        _ => None,
    }
}

/// Generate frontmatter string for a memory file.
pub fn format_frontmatter(mem_type: MemoryType, description: &str, tags: &[String]) -> String {
    use std::fmt::Write;
    let mut fm = format!(
        "---\ntype: {}\ndescription: \"{description}\"\n",
        mem_type.as_str()
    );
    if !tags.is_empty() {
        let _ = writeln!(fm, "tags: [{}]", tags.join(", "));
    }
    fm.push_str("---\n");
    fm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_frontmatter() {
        let content = "---\ntype: decision\ndescription: \"Use Rust for CLI\"\ntags: [lang, arch]\n---\n\n# Decision\nWe chose Rust.";
        let (fm, body) = parse_frontmatter(content);
        let fields = fm.unwrap();
        assert_eq!(fields.memory_type, Some(MemoryType::Decision));
        assert_eq!(fields.description.as_deref(), Some("Use Rust for CLI"));
        assert_eq!(fields.tags, vec!["lang", "arch"]);
        assert!(body.contains("# Decision"));
    }

    #[test]
    fn parse_minimal_frontmatter() {
        let content = "---\ntype: preference\n---\nContent here.";
        let (fm, body) = parse_frontmatter(content);
        let fields = fm.unwrap();
        assert_eq!(fields.memory_type, Some(MemoryType::Preference));
        assert!(fields.description.is_none());
        assert!(fields.tags.is_empty());
        assert_eq!(body, "Content here.");
    }

    #[test]
    fn parse_no_frontmatter() {
        let content = "# Just markdown\nNo frontmatter here.";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn parse_invalid_yaml() {
        let content = "---\n{{{bad yaml\n---\nBody.";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn parse_unclosed_frontmatter() {
        let content = "---\ntype: context\nno closing marker";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn parse_unknown_type() {
        let content = "---\ntype: mystery\n---\nBody.";
        let (fm, _body) = parse_frontmatter(content);
        let fields = fm.unwrap();
        assert!(fields.memory_type.is_none()); // Unknown type → None
    }

    #[test]
    fn memory_type_display() {
        assert_eq!(MemoryType::Decision.to_string(), "decision");
        assert_eq!(MemoryType::Context.to_string(), "context");
        assert_eq!(MemoryType::Preference.to_string(), "preference");
        assert_eq!(MemoryType::Task.to_string(), "task");
        assert_eq!(MemoryType::Reference.to_string(), "reference");
    }

    #[test]
    fn format_frontmatter_basic() {
        let fm = format_frontmatter(MemoryType::Decision, "Use Rust", &["lang".to_string()]);
        assert!(fm.starts_with("---\n"));
        assert!(fm.ends_with("---\n"));
        assert!(fm.contains("type: decision"));
        assert!(fm.contains("description: \"Use Rust\""));
        assert!(fm.contains("tags: [lang]"));
    }

    #[test]
    fn format_frontmatter_no_tags() {
        let fm = format_frontmatter(MemoryType::Context, "Project info", &[]);
        assert!(!fm.contains("tags:"));
    }

    #[test]
    fn memory_type_serde_roundtrip() {
        let json = serde_json::to_string(&MemoryType::Decision).unwrap();
        assert_eq!(json, "\"decision\"");
        let parsed: MemoryType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, MemoryType::Decision);
    }

    #[test]
    fn all_memory_types_serde_roundtrip() {
        let types = [
            MemoryType::Decision,
            MemoryType::Context,
            MemoryType::Preference,
            MemoryType::Task,
            MemoryType::Reference,
        ];
        for t in types {
            let json = serde_json::to_string(&t).unwrap();
            let parsed: MemoryType = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, t);
        }
    }

    #[test]
    fn parse_case_insensitive_type() {
        let content = "---\ntype: DECISION\n---\nBody.";
        let (fm, _) = parse_frontmatter(content);
        let fields = fm.unwrap();
        assert_eq!(fields.memory_type, Some(MemoryType::Decision));
    }

    #[test]
    fn parse_whitespace_around_frontmatter() {
        let content = "  \n---\ntype: context\n---\nBody.";
        let (fm, _) = parse_frontmatter(content);
        let fields = fm.unwrap();
        assert_eq!(fields.memory_type, Some(MemoryType::Context));
    }

    #[test]
    fn format_frontmatter_multiple_tags() {
        let tags = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let fm = format_frontmatter(MemoryType::Reference, "Links", &tags);
        assert!(fm.contains("tags: [a, b, c]"));
    }

    #[test]
    fn parse_all_types() {
        assert_eq!(parse_memory_type("decision"), Some(MemoryType::Decision));
        assert_eq!(parse_memory_type("context"), Some(MemoryType::Context));
        assert_eq!(
            parse_memory_type("preference"),
            Some(MemoryType::Preference)
        );
        assert_eq!(parse_memory_type("task"), Some(MemoryType::Task));
        assert_eq!(parse_memory_type("reference"), Some(MemoryType::Reference));
        assert_eq!(parse_memory_type("unknown"), None);
    }
}
