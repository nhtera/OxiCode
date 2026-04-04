/// Bundled skills embedded at compile time via `include_str!()`.
///
/// These ship with OxiCode and are always available. User or project skills
/// with the same name take precedence (override bundled ones).
use std::path::PathBuf;

use crate::parser::{parse_skill, Skill};

/// Embedded SKILL.md content for each bundled skill.
static BUNDLED_SKILLS: &[(&str, &str)] = &[
    (
        "debug",
        include_str!("../../../assets/skills/debug/SKILL.md"),
    ),
    (
        "remember",
        include_str!("../../../assets/skills/remember/SKILL.md"),
    ),
    (
        "simplify",
        include_str!("../../../assets/skills/simplify/SKILL.md"),
    ),
    (
        "verify",
        include_str!("../../../assets/skills/verify/SKILL.md"),
    ),
    ("loop", include_str!("../../../assets/skills/loop/SKILL.md")),
    (
        "schedule",
        include_str!("../../../assets/skills/schedule/SKILL.md"),
    ),
    (
        "batch",
        include_str!("../../../assets/skills/batch/SKILL.md"),
    ),
];

/// Load all bundled skills from embedded content.
///
/// Returns parsed `Skill` values with synthetic source paths like `<bundled>/debug/SKILL.md`.
/// Parse failures are logged and skipped.
pub fn bundled_skills() -> Vec<Skill> {
    BUNDLED_SKILLS
        .iter()
        .filter_map(|(name, content)| {
            let source_path = PathBuf::from(format!("<bundled>/{name}/SKILL.md"));
            match parse_skill(content, source_path) {
                Ok(skill) => {
                    tracing::debug!("Loaded bundled skill '{}'", skill.name());
                    Some(skill)
                }
                Err(e) => {
                    tracing::warn!("Failed to parse bundled skill '{name}': {e}");
                    None
                }
            }
        })
        .collect()
}

/// Number of bundled skills compiled into the binary.
pub fn bundled_skill_count() -> usize {
    BUNDLED_SKILLS.len()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundled_skills_all_parse_successfully() {
        let skills = bundled_skills();
        assert_eq!(
            skills.len(),
            BUNDLED_SKILLS.len(),
            "all bundled skills should parse without error"
        );
    }

    #[test]
    fn test_bundled_skills_have_correct_names() {
        let skills = bundled_skills();
        let names: Vec<&str> = skills.iter().map(|s| s.name()).collect();
        assert!(names.contains(&"debug"));
        assert!(names.contains(&"remember"));
        assert!(names.contains(&"simplify"));
        assert!(names.contains(&"verify"));
        assert!(names.contains(&"loop"));
        assert!(names.contains(&"schedule"));
        assert!(names.contains(&"batch"));
    }

    #[test]
    fn test_bundled_skills_have_synthetic_paths() {
        let skills = bundled_skills();
        for skill in &skills {
            assert!(
                skill.source_path.starts_with("<bundled>"),
                "bundled skill '{}' should have <bundled> prefix, got {:?}",
                skill.name(),
                skill.source_path
            );
        }
    }

    #[test]
    fn test_bundled_skills_have_non_empty_prompts() {
        let skills = bundled_skills();
        for skill in &skills {
            assert!(
                !skill.prompt.is_empty(),
                "bundled skill '{}' has empty prompt",
                skill.name()
            );
        }
    }

    #[test]
    fn test_bundled_skill_count() {
        assert_eq!(bundled_skill_count(), 7);
    }
}
