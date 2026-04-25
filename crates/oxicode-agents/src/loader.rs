//! Filesystem-defined sub-agent loader.
//!
//! Discovers agent definitions from `~/.oxicode/agents/*.md` and project
//! `.oxicode/agents/*.md` (OxiCode). During transition we also discover
//! `~/.claude/agents/*.md` and `.claude/agents/*.md` for backward-compat.
//! Each `.md` file has a YAML
//! frontmatter block with `name`, `description`, optional `tools` and
//! `model`, followed by the system prompt body.
//!
//! Example agent file (`~/.claude/agents/researcher.md`):
//! ```text
//! ---
//! name: researcher
//! description: Codebase research and exploration
//! tools: Read, Grep, Glob
//! model: claude-haiku-4-5
//! ---
//!
//! You are a careful researcher. Always cite file:line.
//! ```

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// A user-defined sub-agent loaded from a markdown file with YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomAgent {
    /// Agent slug used as `subagent_type` argument (e.g. "researcher").
    pub name: String,
    /// One-line "when to use" description (surfaced in tool schema).
    pub description: String,
    /// Optional whitelist of tool names. `None` = all tools allowed.
    pub tools: Option<Vec<String>>,
    /// Optional model override (defaults to manager's choice).
    pub model: Option<String>,
    /// System prompt body (everything after the closing `---`).
    pub system_prompt: String,
    /// File the agent was loaded from (for error messages / re-loading).
    pub source_path: PathBuf,
}

/// In-process cache so each tool invocation doesn't re-read the filesystem.
static DISCOVERED: OnceLock<Vec<CustomAgent>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentOrigin {
    ProjectOxiCode,
    ProjectClaude,
    UserOxiCode,
    UserClaude,
}

#[derive(Debug, Clone)]
pub struct AgentsByOrigin {
    pub project_oxicode: Vec<CustomAgent>,
    pub project_claude: Vec<CustomAgent>,
    pub user_oxicode: Vec<CustomAgent>,
    pub user_claude: Vec<CustomAgent>,
}

pub fn refresh_with_origins() -> AgentsByOrigin {
    discover_uncached_with_origins(std::env::current_dir().ok().as_deref())
}

pub fn discover_uncached_with_origins(cwd: Option<&Path>) -> AgentsByOrigin {
    let mut out = AgentsByOrigin {
        project_oxicode: Vec::new(),
        project_claude: Vec::new(),
        user_oxicode: Vec::new(),
        user_claude: Vec::new(),
    };

    if let Some(home) = dirs::home_dir() {
        load_dir_into(&home.join(".oxicode").join("agents"), &mut out.user_oxicode);
        load_dir_into(&home.join(".claude").join("agents"), &mut out.user_claude);
    }

    if let Some(cwd) = cwd {
        load_dir_into(
            &cwd.join(".oxicode").join("agents"),
            &mut out.project_oxicode,
        );
        load_dir_into(&cwd.join(".claude").join("agents"), &mut out.project_claude);
    }

    out
}

/// Discover all custom agents (cached on first call). Project-local agents
/// take precedence over user-global ones with the same `name`.
pub fn discover() -> &'static Vec<CustomAgent> {
    DISCOVERED.get_or_init(|| discover_uncached(std::env::current_dir().ok().as_deref()))
}

/// Force a re-scan (for tests, or after a `/agents` create/edit).
pub fn refresh() -> Vec<CustomAgent> {
    discover_uncached(std::env::current_dir().ok().as_deref())
}

/// Scan the user-global and project agent directories, parse each `.md`,
/// and return a deduplicated list (project wins on name collision).
pub fn discover_uncached(cwd: Option<&Path>) -> Vec<CustomAgent> {
    let mut found: Vec<CustomAgent> = Vec::new();

    // User-global: ~/.oxicode/agents/*.md
    if let Some(home) = dirs::home_dir() {
        let user_dir = home.join(".oxicode").join("agents");
        load_dir_into(&user_dir, &mut found);

        // Back-compat: ~/.claude/agents/*.md
        let legacy_user_dir = home.join(".claude").join("agents");
        load_dir_into(&legacy_user_dir, &mut found);
    }

    // Project-local: .oxicode/agents/*.md (relative to cwd, takes precedence).
    if let Some(cwd) = cwd {
        let project_dir = cwd.join(".oxicode").join("agents");
        load_dir_into(&project_dir, &mut found);

        // Back-compat: .claude/agents/*.md
        let legacy_project_dir = cwd.join(".claude").join("agents");
        load_dir_into(&legacy_project_dir, &mut found);
    }

    // De-dupe by name with deterministic precedence:
    // project .oxicode > project .claude > user .oxicode > user .claude.
    let mut seen = std::collections::HashSet::new();
    found
        .into_iter()
        .filter(|a| seen.insert(a.name.clone()))
        .collect()
}

/// Look up a discovered custom agent by name (case-sensitive).
pub fn find_by_name(name: &str) -> Option<CustomAgent> {
    discover().iter().find(|a| a.name == name).cloned()
}

fn load_dir_into(dir: &Path, out: &mut Vec<CustomAgent>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        match load_agent_file(&path) {
            Ok(agent) => out.push(agent),
            Err(e) => tracing::warn!("Failed to load agent {path:?}: {e}"),
        }
    }
}

/// Parse a single agent markdown file into a `CustomAgent`.
pub fn load_agent_file(path: &Path) -> Result<CustomAgent, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (frontmatter, body) = split_frontmatter(&content)
        .ok_or_else(|| format!("missing YAML frontmatter in {}", path.display()))?;
    let meta: AgentFrontmatter = serde_yaml::from_str(frontmatter)
        .map_err(|e| format!("invalid YAML in {}: {e}", path.display()))?;

    Ok(CustomAgent {
        name: meta.name,
        description: meta.description.unwrap_or_default(),
        tools: parse_tools_field(meta.tools.as_deref()),
        model: meta.model,
        system_prompt: body.trim().to_string(),
        source_path: path.to_path_buf(),
    })
}

/// Frontmatter shape (only the fields we care about — extra keys ignored).
#[derive(Debug, Deserialize)]
struct AgentFrontmatter {
    name: String,
    #[serde(default)]
    description: Option<String>,
    /// Either a comma-separated string (`"Read, Grep"`) or a YAML list. We
    /// receive it as a string post-parse and split below.
    #[serde(default)]
    tools: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

/// Split `---\n<yaml>\n---\n<body>` into (yaml, body). Returns None if the
/// file doesn't begin with a frontmatter delimiter.
fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let stripped = content.strip_prefix("---")?;
    let stripped = stripped.trim_start_matches('\r').trim_start_matches('\n');
    let close_idx = stripped.find("\n---")?;
    let yaml = &stripped[..close_idx];
    let after = &stripped[close_idx + 4..];
    let body = after.trim_start_matches('\r').trim_start_matches('\n');
    Some((yaml, body))
}

/// Parse a `tools: a, b, c` string into a vec. Returns None for `*`/`all`/empty.
fn parse_tools_field(raw: Option<&str>) -> Option<Vec<String>> {
    let raw = raw?.trim();
    if raw.is_empty() || raw == "*" || raw.eq_ignore_ascii_case("all") {
        return None;
    }
    let v: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_agent_md(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(format!("{name}.md"));
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn parses_minimal_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent_md(
            dir.path(),
            "researcher",
            "---\nname: researcher\ndescription: Research codebases\n---\n\nYou are a researcher.\n",
        );
        let agent = load_agent_file(&path).unwrap();
        assert_eq!(agent.name, "researcher");
        assert_eq!(agent.description, "Research codebases");
        assert_eq!(agent.system_prompt, "You are a researcher.");
        assert!(agent.tools.is_none());
        assert!(agent.model.is_none());
    }

    #[test]
    fn parses_tools_csv() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent_md(
            dir.path(),
            "explorer",
            "---\nname: explorer\ndescription: Explore\ntools: Read, Grep, Glob\nmodel: claude-haiku-4-5\n---\n\nBody.",
        );
        let agent = load_agent_file(&path).unwrap();
        let tools = agent.tools.expect("tools should be Some");
        assert_eq!(tools, vec!["Read", "Grep", "Glob"]);
        assert_eq!(agent.model.as_deref(), Some("claude-haiku-4-5"));
    }

    #[test]
    fn star_tools_means_all() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent_md(
            dir.path(),
            "general",
            "---\nname: general\ndescription: All tools\ntools: \"*\"\n---\n\nBody.",
        );
        let agent = load_agent_file(&path).unwrap();
        assert!(agent.tools.is_none());
    }

    #[test]
    fn missing_frontmatter_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent_md(
            dir.path(),
            "no_fm",
            "Just a system prompt with no frontmatter.",
        );
        assert!(load_agent_file(&path).is_err());
    }

    #[test]
    fn discover_uncached_picks_up_files() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();
        let agents_dir = cwd.join(".oxicode").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        // Back-compat: legacy directory also works.
        let legacy_agents_dir = cwd.join(".claude").join("agents");
        std::fs::create_dir_all(&legacy_agents_dir).unwrap();
        write_agent_md(
            &agents_dir,
            "alpha",
            "---\nname: alpha\ndescription: A\n---\nA body.",
        );
        write_agent_md(
            &legacy_agents_dir,
            "beta",
            "---\nname: beta\ndescription: B\n---\nB body.",
        );
        let agents = discover_uncached(Some(cwd));
        let names: Vec<_> = agents.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }
}
