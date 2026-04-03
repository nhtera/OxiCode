//! Auto-detect project type by scanning for well-known config files.
//!
//! Returns the appropriate command strings for test, lint, format, build, and deploy.

use std::path::Path;

/// Detected project type with command mappings.
#[derive(Debug, Clone)]
pub struct ProjectCommands {
    pub project_type: &'static str,
    pub test: (&'static str, Vec<&'static str>),
    pub lint: (&'static str, Vec<&'static str>),
    pub format: (&'static str, Vec<&'static str>),
    pub build: (&'static str, Vec<&'static str>),
    pub deploy: Option<(&'static str, Vec<&'static str>)>,
}

/// Detection rule: (config_file, project_type, commands).
struct DetectionRule {
    config_file: &'static str,
    commands: ProjectCommands,
}

/// All detection rules ordered by specificity (most specific first).
#[allow(clippy::too_many_lines)]
fn detection_rules() -> Vec<DetectionRule> {
    vec![
        DetectionRule {
            config_file: "Cargo.toml",
            commands: ProjectCommands {
                project_type: "rust",
                test: ("cargo", vec!["test"]),
                lint: ("cargo", vec!["clippy", "--all-targets"]),
                format: ("cargo", vec!["fmt"]),
                build: ("cargo", vec!["build"]),
                deploy: None,
            },
        },
        DetectionRule {
            config_file: "package.json",
            commands: ProjectCommands {
                project_type: "node",
                test: ("npm", vec!["test"]),
                lint: ("npx", vec!["eslint", "."]),
                format: ("npx", vec!["prettier", "--write", "."]),
                build: ("npm", vec!["run", "build"]),
                deploy: None,
            },
        },
        DetectionRule {
            config_file: "pyproject.toml",
            commands: ProjectCommands {
                project_type: "python",
                test: ("python", vec!["-m", "pytest"]),
                lint: ("python", vec!["-m", "ruff", "check", "."]),
                format: ("python", vec!["-m", "ruff", "format", "."]),
                build: ("python", vec!["-m", "build"]),
                deploy: None,
            },
        },
        DetectionRule {
            config_file: "setup.py",
            commands: ProjectCommands {
                project_type: "python",
                test: ("python", vec!["-m", "pytest"]),
                lint: ("python", vec!["-m", "flake8", "."]),
                format: ("python", vec!["-m", "black", "."]),
                build: ("python", vec!["setup.py", "build"]),
                deploy: None,
            },
        },
        DetectionRule {
            config_file: "go.mod",
            commands: ProjectCommands {
                project_type: "go",
                test: ("go", vec!["test", "./..."]),
                lint: ("golangci-lint", vec!["run"]),
                format: ("gofmt", vec!["-w", "."]),
                build: ("go", vec!["build", "./..."]),
                deploy: None,
            },
        },
        DetectionRule {
            config_file: "pom.xml",
            commands: ProjectCommands {
                project_type: "java-maven",
                test: ("mvn", vec!["test"]),
                lint: ("mvn", vec!["checkstyle:check"]),
                format: ("mvn", vec!["spotless:apply"]),
                build: ("mvn", vec!["package"]),
                deploy: None,
            },
        },
        DetectionRule {
            config_file: "build.gradle",
            commands: ProjectCommands {
                project_type: "java-gradle",
                test: ("./gradlew", vec!["test"]),
                lint: ("./gradlew", vec!["check"]),
                format: ("./gradlew", vec!["spotlessApply"]),
                build: ("./gradlew", vec!["build"]),
                deploy: None,
            },
        },
        DetectionRule {
            config_file: "Makefile",
            commands: ProjectCommands {
                project_type: "make",
                test: ("make", vec!["test"]),
                lint: ("make", vec!["lint"]),
                format: ("make", vec!["format"]),
                build: ("make", vec!["build"]),
                deploy: Some(("make", vec!["deploy"])),
            },
        },
        DetectionRule {
            config_file: "CMakeLists.txt",
            commands: ProjectCommands {
                project_type: "cmake",
                test: ("cmake", vec!["--build", "build", "--target", "test"]),
                lint: ("cmake", vec!["--build", "build", "--target", "lint"]),
                format: ("clang-format", vec!["-i", "-style=file"]),
                build: ("cmake", vec!["--build", "build"]),
                deploy: None,
            },
        },
        DetectionRule {
            config_file: "deno.json",
            commands: ProjectCommands {
                project_type: "deno",
                test: ("deno", vec!["test"]),
                lint: ("deno", vec!["lint"]),
                format: ("deno", vec!["fmt"]),
                build: ("deno", vec!["compile"]),
                deploy: Some(("deno", vec!["deploy"])),
            },
        },
    ]
}

/// Detect project type from the current working directory.
///
/// Walks up from `start_dir` to find a known config file (max 10 levels).
/// Returns `None` if no project is detected.
pub fn detect_project(start_dir: &Path) -> Option<ProjectCommands> {
    let rules = detection_rules();
    let mut dir = start_dir.to_path_buf();

    for _ in 0..10 {
        for rule in &rules {
            if dir.join(rule.config_file).exists() {
                // Check for deploy config overrides at this level.
                let mut cmds = rule.commands.clone();
                if cmds.deploy.is_none() {
                    cmds.deploy = detect_deploy_config(&dir);
                }
                return Some(cmds);
            }
        }

        if dir.join(".git").exists() {
            break; // Stop at git root.
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Check for deploy-specific config files.
fn detect_deploy_config(dir: &Path) -> Option<(&'static str, Vec<&'static str>)> {
    if dir.join("fly.toml").exists() {
        return Some(("fly", vec!["deploy"]));
    }
    if dir.join("vercel.json").exists() {
        return Some(("vercel", vec!["deploy"]));
    }
    if dir.join("netlify.toml").exists() {
        return Some(("netlify", vec!["deploy"]));
    }
    if dir.join("Dockerfile").exists() {
        return Some(("docker", vec!["build", "-t", "app", "."]));
    }
    if dir.join("railway.toml").exists() || dir.join("railway.json").exists() {
        return Some(("railway", vec!["up"]));
    }
    None
}

/// Format a command tuple as a human-readable string for display.
pub fn format_cmd(cmd: &str, args: &[&str]) -> String {
    if args.is_empty() {
        cmd.to_string()
    } else {
        format!("{cmd} {}", args.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_project_rust() {
        // This test runs inside a Cargo project.
        let cwd = std::env::current_dir().unwrap();
        let result = detect_project(&cwd);
        assert!(result.is_some());
        let cmds = result.unwrap();
        assert_eq!(cmds.project_type, "rust");
        assert_eq!(cmds.test.0, "cargo");
    }

    #[test]
    fn test_detect_project_none() {
        // Temp dir with no config files.
        let tmp = std::env::temp_dir().join("oxicode_test_empty_proj");
        let _ = std::fs::create_dir_all(&tmp);
        // Create .git so we stop at this level.
        let _ = std::fs::create_dir_all(tmp.join(".git"));
        let result = detect_project(&tmp);
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_format_cmd() {
        assert_eq!(format_cmd("cargo", &["test"]), "cargo test");
        assert_eq!(format_cmd("npm", &["run", "build"]), "npm run build");
        assert_eq!(format_cmd("make", &[]), "make");
    }

    #[test]
    fn test_detection_rules_count() {
        let rules = detection_rules();
        assert!(rules.len() >= 8, "Should have at least 8 project types");
    }
}
