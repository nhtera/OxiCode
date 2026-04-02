//! Skill file discovery.
//!
//! Walks configured directories to find `SKILL.md` files, parses each one,
//! and deduplicates by skill name (project skills override user skills).

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

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

    /// Discover all skills from both directories.
    ///
    /// Project skills take precedence: if the same skill `name` appears in
    /// both directories, the project version replaces the user version.
    pub fn discover(&self) -> Vec<Skill> {
        let mut skills: Vec<Skill> = Vec::new();

        // Load user skills first.
        for skill in Self::discover_from_dir(&self.user_dir) {
            skills.push(skill);
        }

        // Load project skills, replacing any user skill with the same name.
        for skill in Self::discover_from_dir(&self.project_dir) {
            if let Some(existing) = skills.iter_mut().find(|s| s.name() == skill.name()) {
                tracing::debug!(
                    "Project skill '{}' overrides user skill at {:?}",
                    skill.name(),
                    existing.source_path
                );
                *existing = skill;
            } else {
                skills.push(skill);
            }
        }

        tracing::debug!("Discovered {} skill(s) total", skills.len());
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

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].metadata.description, "project version");
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
