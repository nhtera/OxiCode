//! SKILL.md parser.
//!
//! Parses markdown files with YAML frontmatter into `Skill` structs.
//! Frontmatter is delimited by `---` lines; the body after the second `---`
//! is the skill prompt. YAML is parsed manually (no external YAML crate).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use oxicode_common::{OxiError, OxiResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Where the skill prompt is injected in the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum InjectMode {
    #[default]
    System,
    User,
}

/// Conditions under which a skill activates automatically.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActivationRule {
    /// Glob patterns matched against the current file path.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Keywords matched (case-insensitively) against user input.
    #[serde(default)]
    pub keywords: Vec<String>,
}

impl ActivationRule {
    /// Returns `true` when no paths and no keywords are configured.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.keywords.is_empty()
    }
}

/// Parsed frontmatter from a SKILL.md file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub activation: ActivationRule,
    #[serde(default)]
    pub inject: InjectMode,
}

/// A fully parsed skill: metadata + prompt body + origin path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub metadata: SkillMetadata,
    /// The raw prompt text that follows the frontmatter.
    pub prompt: String,
    /// Filesystem path this skill was loaded from.
    pub source_path: PathBuf,
}

impl Skill {
    pub fn name(&self) -> &str {
        &self.metadata.name
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a SKILL.md `content` string into a [`Skill`].
///
/// Expects the format:
/// ```text
/// ---
/// name: my-skill
/// description: Does something
/// ---
/// Prompt body here…
/// ```
pub fn parse_skill(content: &str, source_path: PathBuf) -> OxiResult<Skill> {
    let (frontmatter, body) = split_frontmatter(content)?;
    let metadata = parse_frontmatter(frontmatter, &source_path)?;
    let prompt = body.trim().to_string();

    Ok(Skill {
        metadata,
        prompt,
        source_path,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Split content on `---` delimiters. Returns `(frontmatter, body)`.
fn split_frontmatter(content: &str) -> OxiResult<(&str, &str)> {
    // The file must start with `---` (optionally preceded by whitespace/BOM).
    let content = content.trim_start_matches('\u{feff}'); // strip BOM if present
    let after_first = content
        .trim_start()
        .strip_prefix("---")
        .ok_or_else(|| OxiError::Other("SKILL.md missing opening '---' delimiter".to_string()))?;

    // Skip the newline immediately after `---`
    let after_first = after_first
        .strip_prefix('\n')
        .or_else(|| after_first.strip_prefix("\r\n"))
        .unwrap_or(after_first);

    // Find the closing `---`
    let close_marker = "\n---";
    let close_pos = after_first
        .find(close_marker)
        .ok_or_else(|| OxiError::Other("SKILL.md missing closing '---' delimiter".to_string()))?;

    let frontmatter = &after_first[..close_pos];
    let rest = &after_first[close_pos + close_marker.len()..];

    // Strip optional newline right after closing `---`
    let body = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))
        .unwrap_or(rest);

    Ok((frontmatter, body))
}

/// Parse a simple `key: value` / `key: [a, b]` YAML block into `SkillMetadata`.
fn parse_frontmatter(fm: &str, source_path: &PathBuf) -> OxiResult<SkillMetadata> {
    let mut name = String::new();
    let mut description = String::new();
    let mut inject = InjectMode::System;
    let mut activation = ActivationRule::default();

    // State machine for multi-line `activation:` block.
    let mut in_activation = false;
    let mut in_paths = false;
    let mut in_keywords = false;

    for raw_line in fm.lines() {
        let line = raw_line.trim_end();

        // Detect top-level keys (no leading spaces).
        if !raw_line.starts_with(' ') && !raw_line.starts_with('\t') {
            in_activation = false;
            in_paths = false;
            in_keywords = false;

            if let Some(rest) = line.strip_prefix("name:") {
                name = strip_value(rest);
            } else if let Some(rest) = line.strip_prefix("description:") {
                description = strip_value(rest);
            } else if let Some(rest) = line.strip_prefix("inject:") {
                let v = strip_value(rest);
                inject = if v.eq_ignore_ascii_case("user") {
                    InjectMode::User
                } else {
                    InjectMode::System
                };
            } else if line.trim_start() == "activation:" || line.starts_with("activation:") {
                in_activation = true;
                // Inline list on same line? e.g. `activation: {}` — ignore, handled below.
                let rest = line.strip_prefix("activation:").unwrap_or("").trim();
                if !rest.is_empty() && rest != "{}" {
                    // Not handled: complex inline — log and skip.
                    tracing::debug!("Skipping inline activation value in {:?}", source_path);
                    in_activation = false;
                }
            }
            continue;
        }

        // Inside `activation:` block — look for `paths:` and `keywords:`.
        if in_activation {
            let trimmed = line.trim();

            if let Some(rest) = trimmed.strip_prefix("paths:") {
                in_paths = true;
                in_keywords = false;
                // Inline list: `paths: ["*.rs", "Cargo.toml"]`
                let rest = rest.trim();
                if rest.starts_with('[') {
                    activation.paths = parse_inline_list(rest);
                    in_paths = false;
                }
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("keywords:") {
                in_keywords = true;
                in_paths = false;
                let rest = rest.trim();
                if rest.starts_with('[') {
                    activation.keywords = parse_inline_list(rest);
                    in_keywords = false;
                }
                continue;
            }

            // `- item` list entries
            if let Some(rest) = trimmed.strip_prefix("- ") {
                let item = strip_quotes(rest.trim());
                if in_paths {
                    activation.paths.push(item);
                } else if in_keywords {
                    activation.keywords.push(item);
                }
            }
        }
    }

    if name.is_empty() {
        return Err(OxiError::Other(format!(
            "SKILL.md at {} missing required field 'name'",
            source_path.display()
        )));
    }

    Ok(SkillMetadata {
        name,
        description,
        activation,
        inject,
    })
}

/// Strip leading/trailing whitespace and surrounding quotes from a YAML value.
fn strip_value(s: &str) -> String {
    strip_quotes(s.trim())
}

/// Remove surrounding single or double quotes.
fn strip_quotes(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Parse an inline YAML list `["a", "b", c]` into a `Vec<String>`.
fn parse_inline_list(s: &str) -> Vec<String> {
    let inner = s.trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|item| strip_quotes(item.trim()))
        .filter(|s| !s.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn skill_path() -> PathBuf {
        PathBuf::from("test/SKILL.md")
    }

    #[test]
    fn test_parse_minimal_skill() {
        let content = "---\nname: my-skill\ndescription: A test skill\n---\nDo something useful.\n";
        let skill = parse_skill(content, skill_path()).unwrap();
        assert_eq!(skill.name(), "my-skill");
        assert_eq!(skill.metadata.description, "A test skill");
        assert_eq!(skill.prompt, "Do something useful.");
        assert!(matches!(skill.metadata.inject, InjectMode::System));
        assert!(skill.metadata.activation.is_empty());
    }

    #[test]
    fn test_parse_skill_with_activation_inline_lists() {
        let content = indoc(
            r#"---
name: rust-expert
description: Rust expertise
inject: system
activation:
  paths: ["*.rs", "Cargo.toml"]
  keywords: ["rust", "cargo"]
---
You are a Rust expert.
"#,
        );
        let skill = parse_skill(content, skill_path()).unwrap();
        assert_eq!(skill.name(), "rust-expert");
        assert_eq!(skill.metadata.activation.paths, vec!["*.rs", "Cargo.toml"]);
        assert_eq!(skill.metadata.activation.keywords, vec!["rust", "cargo"]);
    }

    #[test]
    fn test_parse_skill_with_activation_block_lists() {
        let content = indoc(
            r#"---
name: ts-expert
description: TypeScript
inject: user
activation:
  paths:
    - "*.ts"
    - "tsconfig.json"
  keywords:
    - typescript
    - ts
---
Prompt body.
"#,
        );
        let skill = parse_skill(content, skill_path()).unwrap();
        assert_eq!(skill.metadata.inject, InjectMode::User);
        assert_eq!(
            skill.metadata.activation.paths,
            vec!["*.ts", "tsconfig.json"]
        );
        assert_eq!(skill.metadata.activation.keywords, vec!["typescript", "ts"]);
    }

    #[test]
    fn test_missing_name_returns_error() {
        let content = "---\ndescription: No name here\n---\nBody.\n";
        let result = parse_skill(content, skill_path());
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_opening_delimiter_returns_error() {
        let content = "name: skill\n---\nBody.\n";
        let result = parse_skill(content, skill_path());
        assert!(result.is_err());
    }

    #[test]
    fn test_strip_quotes() {
        assert_eq!(strip_quotes("\"hello\""), "hello");
        assert_eq!(strip_quotes("'world'"), "world");
        assert_eq!(strip_quotes("bare"), "bare");
    }

    #[test]
    fn test_parse_inline_list() {
        let items = parse_inline_list(r#"["*.rs", "Cargo.toml"]"#);
        assert_eq!(items, vec!["*.rs", "Cargo.toml"]);
    }

    /// Tiny helper so test strings don't need leading newlines.
    fn indoc(s: &str) -> &str {
        s
    }
}
