//! Conditional skill activation.
//!
//! A skill activates when ANY of its path patterns match the current file,
//! OR when ANY of its keywords appear in the user input (case-insensitive).
//! Skills with empty activation rules are always active (static skills).

use glob::Pattern;

use crate::parser::Skill;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Runtime context used to evaluate activation rules.
#[derive(Debug, Clone, Default)]
pub struct ActivationContext {
    /// Path of the file currently open / being edited.
    pub current_file: Option<String>,
    /// The user's latest input message.
    pub user_input: Option<String>,
}

/// Evaluates activation rules against a runtime context.
pub struct SkillActivator;

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl SkillActivator {
    /// Returns `true` if `skill` should be active given `context`.
    ///
    /// Activation logic:
    /// - Empty rules → always active.
    /// - Non-empty rules → active if ANY path glob matches OR ANY keyword matches.
    pub fn check_activation(skill: &Skill, context: &ActivationContext) -> bool {
        let rule = &skill.metadata.activation;

        // Static skill — no conditions configured.
        if rule.is_empty() {
            return true;
        }

        // Path matching.
        if let Some(ref file) = context.current_file {
            for pattern_str in &rule.paths {
                match Pattern::new(pattern_str) {
                    Ok(pattern) => {
                        // Match against the full path and also just the file name.
                        let file_name = std::path::Path::new(file)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(file.as_str());

                        if pattern.matches(file) || pattern.matches(file_name) {
                            tracing::debug!(
                                "Skill '{}' activated by path pattern '{}' on '{}'",
                                skill.name(),
                                pattern_str,
                                file
                            );
                            return true;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Invalid glob pattern '{}' in skill '{}': {e}",
                            pattern_str,
                            skill.name()
                        );
                    }
                }
            }
        }

        // Keyword matching.
        if let Some(ref input) = context.user_input {
            let input_lower = input.to_lowercase();
            for keyword in &rule.keywords {
                if input_lower.contains(keyword.to_lowercase().as_str()) {
                    tracing::debug!(
                        "Skill '{}' activated by keyword '{}' in user input",
                        skill.name(),
                        keyword
                    );
                    return true;
                }
            }
        }

        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::parser::{ActivationRule, InjectMode, Skill, SkillMetadata};

    fn make_skill(paths: Vec<&str>, keywords: Vec<&str>) -> Skill {
        Skill {
            metadata: SkillMetadata {
                name: "test-skill".to_string(),
                description: String::new(),
                activation: ActivationRule {
                    paths: paths.into_iter().map(String::from).collect(),
                    keywords: keywords.into_iter().map(String::from).collect(),
                },
                inject: InjectMode::System,
            },
            prompt: String::new(),
            source_path: PathBuf::from("SKILL.md"),
        }
    }

    fn make_static_skill() -> Skill {
        make_skill(vec![], vec![])
    }

    #[test]
    fn test_static_skill_always_active() {
        let skill = make_static_skill();
        let ctx = ActivationContext::default();
        assert!(SkillActivator::check_activation(&skill, &ctx));
    }

    #[test]
    fn test_path_glob_matches_extension() {
        let skill = make_skill(vec!["*.rs"], vec![]);
        let ctx = ActivationContext {
            current_file: Some("src/main.rs".to_string()),
            user_input: None,
        };
        assert!(SkillActivator::check_activation(&skill, &ctx));
    }

    #[test]
    fn test_path_glob_no_match_no_keyword() {
        let skill = make_skill(vec!["*.rs"], vec![]);
        let ctx = ActivationContext {
            current_file: Some("index.ts".to_string()),
            user_input: None,
        };
        assert!(!SkillActivator::check_activation(&skill, &ctx));
    }

    #[test]
    fn test_keyword_match_case_insensitive() {
        let skill = make_skill(vec![], vec!["rust"]);
        let ctx = ActivationContext {
            current_file: None,
            user_input: Some("Help me with RUST lifetimes".to_string()),
        };
        assert!(SkillActivator::check_activation(&skill, &ctx));
    }

    #[test]
    fn test_keyword_no_match() {
        let skill = make_skill(vec![], vec!["rust"]);
        let ctx = ActivationContext {
            current_file: None,
            user_input: Some("Help me with Python".to_string()),
        };
        assert!(!SkillActivator::check_activation(&skill, &ctx));
    }

    #[test]
    fn test_path_or_keyword_or_logic() {
        // Skill has both — only keyword matches.
        let skill = make_skill(vec!["*.rs"], vec!["cargo"]);
        let ctx = ActivationContext {
            current_file: Some("app.py".to_string()),
            user_input: Some("run cargo build".to_string()),
        };
        assert!(SkillActivator::check_activation(&skill, &ctx));
    }

    #[test]
    fn test_filename_only_glob_match() {
        let skill = make_skill(vec!["Cargo.toml"], vec![]);
        let ctx = ActivationContext {
            current_file: Some("/home/user/project/Cargo.toml".to_string()),
            user_input: None,
        };
        assert!(SkillActivator::check_activation(&skill, &ctx));
    }
}
