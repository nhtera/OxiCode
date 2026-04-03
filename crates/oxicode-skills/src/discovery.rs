//! Skill file discovery.
//!
//! Walks configured directories to find `SKILL.md` files, parses each one,
//! and deduplicates by skill name (project skills override user skills).

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::bundled::bundled_skills;
use crate::parser::{parse_skill, Skill};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Discovers skills from a user-level and a project-level directory.
pub struct SkillDiscovery {
    user_dir: PathBuf,
    project_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl SkillDiscovery {
    /// Create a new discoverer with separate user and project skill directories.
    pub fn new(user_dir: PathBuf, project_dir: PathBuf) -> Self {
        Self {
            user_dir,
            project_dir,
        }
    }

    /// Discover all skills: bundled (lowest priority) → user → project (highest priority).
    ///
    /// If the same skill `name` appears at multiple levels, the higher-priority
    /// version replaces the lower one.
    pub fn discover(&self) -> Vec<Skill> {
        // Start with bundled skills (lowest priority).
        let bundled = bundled_skills();
        let bundled_count = bundled.len();
        let mut skills: Vec<Skill> = bundled;

        // Load user skills, overriding bundled skills with same name.
        for skill in Self::discover_from_dir(&self.user_dir) {
            if let Some(existing) = skills.iter_mut().find(|s| s.name() == skill.name()) {
                tracing::debug!(
                    "User skill '{}' overrides bundled skill",
                    skill.name(),
                );
                *existing = skill;
            } else {
                skills.push(skill);
            }
        }

        // Load project skills, overriding user/bundled skills with same name.
        for skill in Self::discover_from_dir(&self.project_dir) {
            if let Some(existing) = skills.iter_mut().find(|s| s.name() == skill.name()) {
                tracing::debug!(
                    "Project skill '{}' overrides skill at {:?}",
                    skill.name(),
                    existing.source_path
                );
                *existing = skill;
            } else {
                skills.push(skill);
            }
        }

        tracing::debug!("Discovered {} skill(s) total ({} bundled)", skills.len(), bundled_count);
        skills
    }

    /// Scan a single directory recursively for `SKILL.md` files and parse each.
    ///
    /// Parse failures are logged as warnings and skipped; they do not abort
    /// the whole discovery run.
    pub fn discover_from_dir(dir: &Path) -> Vec<Skill> {
        if !dir.exists() {
            tracing::debug!("Skill directory does not exist, skipping: {:?}", dir);
            return Vec::new();
        }

        let mut skills = Vec::new();

        for entry in WalkDir::new(dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| {
                e.map_err(|err| {
                    tracing::warn!("Error walking skill directory: {err}");
                })
                .ok()
            })
        {
            if !entry.file_type().is_file() {
                continue;
            }

            // Match `SKILL.md` case-insensitively.
            let file_name = entry.file_name().to_string_lossy().to_lowercase();
            if file_name != "skill.md" {
                continue;
            }

            let path = entry.into_path();
            match std::fs::read_to_string(&path) {
                Ok(content) => match parse_skill(&content, path.clone()) {
                    Ok(skill) => {
                        tracing::debug!("Loaded skill '{}' from {:?}", skill.name(), path);
                        skills.push(skill);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse skill at {:?}: {e}", path);
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to read skill file {:?}: {e}", path);
                }
            }
        }

        skills
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_skill(dir: &Path, subdir: &str, content: &str) {
        let skill_dir = dir.join(subdir);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn test_discover_from_nonexistent_dir_returns_empty() {
        let skills = SkillDiscovery::discover_from_dir(Path::new("/nonexistent/path/xyz"));
        assert!(skills.is_empty());
    }

    #[test]
    fn test_discover_single_skill() {
        let tmp = tempdir();
        write_skill(
            &tmp,
            "rust-expert",
            "---\nname: rust-expert\ndescription: Rust\n---\nBe a Rust expert.\n",
        );
        let skills = SkillDiscovery::discover_from_dir(&tmp);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name(), "rust-expert");
    }

    #[test]
    fn test_project_skill_overrides_user_skill() {
        let user_tmp = tempdir();
        let project_tmp = tempdir();

        write_skill(
            &user_tmp,
            "my-skill",
            "---\nname: my-skill\ndescription: user version\n---\nUser prompt.\n",
        );
        write_skill(
            &project_tmp,
            "my-skill",
            "---\nname: my-skill\ndescription: project version\n---\nProject prompt.\n",
        );

        let discovery = SkillDiscovery::new(user_tmp.clone(), project_tmp.clone());
        let skills = discovery.discover();

        // Should have bundled skills + 1 user-defined (project overrides user).
        let my_skill = skills.iter().find(|s| s.name() == "my-skill").unwrap();
        assert_eq!(my_skill.metadata.description, "project version");
    }

    #[test]
    fn test_discover_includes_bundled_skills() {
        let user_tmp = tempdir();
        let project_tmp = tempdir();
        let discovery = SkillDiscovery::new(user_tmp, project_tmp);
        let skills = discovery.discover();

        // Should include all bundled skills even with empty directories.
        assert!(skills.iter().any(|s| s.name() == "debug"));
        assert!(skills.iter().any(|s| s.name() == "simplify"));
        assert!(skills.iter().any(|s| s.name() == "batch"));
    }

    #[test]
    fn test_user_skill_overrides_bundled_skill() {
        let user_tmp = tempdir();
        let project_tmp = tempdir();

        // Override the bundled "debug" skill with a user version.
        write_skill(
            &user_tmp,
            "debug",
            "---\nname: debug\ndescription: custom debug\n---\nCustom debug prompt.\n",
        );

        let discovery = SkillDiscovery::new(user_tmp, project_tmp);
        let skills = discovery.discover();

        let debug_skill = skills.iter().find(|s| s.name() == "debug").unwrap();
        assert_eq!(debug_skill.metadata.description, "custom debug");
    }

    #[test]
    fn test_invalid_skill_file_is_skipped() {
        let tmp = tempdir();
        let skill_dir = tmp.join("bad-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        // No `---` delimiter — parse will fail.
        fs::write(skill_dir.join("SKILL.md"), "not valid frontmatter").unwrap();

        let skills = SkillDiscovery::discover_from_dir(&tmp);
        assert!(skills.is_empty());
    }

    /// Create a temporary directory that auto-cleans via drop (simple version).
    fn tempdir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "oxicode-skills-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
