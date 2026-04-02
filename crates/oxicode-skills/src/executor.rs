//! Skill prompt injection executor.
//!
//! Holds discovered skills and, given an `ActivationContext`, returns the
//! subset that are active and assembles them into a combined prompt block.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::activation::{ActivationContext, SkillActivator};
use crate::parser::Skill;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Lightweight summary of a skill for listing / UI purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub source_path: PathBuf,
    /// `true` when the skill has no activation rules (always active).
    pub is_static: bool,
}

/// Holds a collection of discovered skills and evaluates them on demand.
pub struct SkillExecutor {
    skills: Vec<Skill>,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl SkillExecutor {
    /// Create an executor from a pre-discovered list of skills.
    pub fn new(skills: Vec<Skill>) -> Self {
        Self { skills }
    }

    /// Return references to all skills that are active for `context`.
    pub fn get_active_skills<'a>(&'a self, context: &ActivationContext) -> Vec<&'a Skill> {
        self.skills
            .iter()
            .filter(|s| SkillActivator::check_activation(s, context))
            .collect()
    }

    /// Build a combined prompt string from all active skills.
    ///
    /// Each skill is formatted as:
    /// ```text
    /// \n# Skill: {name}\n\n{prompt}\n
    /// ```
    /// Returns `None` when no skills are active.
    pub fn build_skills_prompt(&self, context: &ActivationContext) -> Option<String> {
        let active = self.get_active_skills(context);
        if active.is_empty() {
            return None;
        }

        let combined = active.iter().fold(String::new(), |mut acc, s| {
            use std::fmt::Write;
            let _ = writeln!(acc, "\n# Skill: {}\n\n{}\n", s.name(), s.prompt);
            acc
        });

        tracing::debug!("Built skills prompt from {} active skill(s)", active.len());

        Some(combined)
    }

    /// Return lightweight info about every loaded skill.
    pub fn list_skills(&self) -> Vec<SkillInfo> {
        self.skills
            .iter()
            .map(|s| SkillInfo {
                name: s.metadata.name.clone(),
                description: s.metadata.description.clone(),
                source_path: s.source_path.clone(),
                is_static: s.metadata.activation.is_empty(),
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::parser::{ActivationRule, InjectMode, SkillMetadata};

    fn make_skill(name: &str, paths: Vec<&str>, keywords: Vec<&str>, prompt: &str) -> Skill {
        Skill {
            metadata: SkillMetadata {
                name: name.to_string(),
                description: format!("{name} description"),
                activation: ActivationRule {
                    paths: paths.into_iter().map(String::from).collect(),
                    keywords: keywords.into_iter().map(String::from).collect(),
                },
                inject: InjectMode::System,
            },
            prompt: prompt.to_string(),
            source_path: PathBuf::from(format!("{name}/SKILL.md")),
        }
    }

    #[test]
    fn test_no_active_skills_returns_none() {
        let skills = vec![make_skill("rust", vec!["*.rs"], vec![], "Rust prompt.")];
        let executor = SkillExecutor::new(skills);
        let ctx = ActivationContext {
            current_file: Some("app.py".to_string()),
            user_input: None,
        };
        assert!(executor.build_skills_prompt(&ctx).is_none());
    }

    #[test]
    fn test_active_skill_builds_prompt() {
        let skills = vec![make_skill(
            "rust",
            vec!["*.rs"],
            vec![],
            "Be a Rust expert.",
        )];
        let executor = SkillExecutor::new(skills);
        let ctx = ActivationContext {
            current_file: Some("main.rs".to_string()),
            user_input: None,
        };
        let prompt = executor.build_skills_prompt(&ctx).unwrap();
        assert!(prompt.contains("# Skill: rust"));
        assert!(prompt.contains("Be a Rust expert."));
    }

    #[test]
    fn test_multiple_active_skills_joined() {
        let skills = vec![
            make_skill("base", vec![], vec![], "Base prompt."),
            make_skill("rust", vec!["*.rs"], vec![], "Rust prompt."),
        ];
        let executor = SkillExecutor::new(skills);
        let ctx = ActivationContext {
            current_file: Some("lib.rs".to_string()),
            user_input: None,
        };
        let prompt = executor.build_skills_prompt(&ctx).unwrap();
        assert!(prompt.contains("# Skill: base"));
        assert!(prompt.contains("# Skill: rust"));
    }

    #[test]
    fn test_list_skills_info() {
        let skills = vec![
            make_skill("static-skill", vec![], vec![], "Always on."),
            make_skill("conditional", vec!["*.ts"], vec![], "TS only."),
        ];
        let executor = SkillExecutor::new(skills);
        let list = executor.list_skills();
        assert_eq!(list.len(), 2);
        assert!(
            list.iter()
                .find(|i| i.name == "static-skill")
                .unwrap()
                .is_static
        );
        assert!(
            !list
                .iter()
                .find(|i| i.name == "conditional")
                .unwrap()
                .is_static
        );
    }

    #[test]
    fn test_get_active_skills_filters_correctly() {
        let skills = vec![
            make_skill("always", vec![], vec![], "Always."),
            make_skill("rust-only", vec!["*.rs"], vec![], "Rust."),
        ];
        let executor = SkillExecutor::new(skills);
        let ctx = ActivationContext {
            current_file: Some("index.ts".to_string()),
            user_input: None,
        };
        let active = executor.get_active_skills(&ctx);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name(), "always");
    }
}
