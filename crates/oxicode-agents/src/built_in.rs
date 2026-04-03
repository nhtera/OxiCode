/// Built-in agent type definitions with system prompts and tool restrictions.
///
/// Each agent type has a tailored system prompt and a whitelist of tools it can use.
/// When no type is specified, `General` is used (all tools available).
use serde::{Deserialize, Serialize};

/// Specialized agent types with restricted tool access and tailored prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentType {
    /// Implementation planning — read-only tools + write to plans directory.
    Plan,
    /// Codebase research and exploration — read-only + search tools.
    Explore,
    /// Code review and verification — read-only + search + test execution.
    Verify,
    /// General-purpose fallback — all tools available.
    General,
    /// Tutorial and help — read-only tools only.
    Guide,
    /// Terminal/statusline configuration — read + write config files.
    Statusline,
}

impl AgentType {
    /// Parse a string into an AgentType (case-insensitive).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "plan" | "planner" => Some(Self::Plan),
            "explore" | "explorer" | "research" => Some(Self::Explore),
            "verify" | "verification" | "review" => Some(Self::Verify),
            "general" | "general-purpose" => Some(Self::General),
            "guide" | "tutorial" | "help" => Some(Self::Guide),
            "statusline" | "statusline-setup" | "status" => Some(Self::Statusline),
            _ => None,
        }
    }

    /// System prompt injected into subagent conversations.
    pub fn system_prompt(&self) -> &'static str {
        match self {
            Self::Plan => PLAN_SYSTEM_PROMPT,
            Self::Explore => EXPLORE_SYSTEM_PROMPT,
            Self::Verify => VERIFY_SYSTEM_PROMPT,
            Self::General => GENERAL_SYSTEM_PROMPT,
            Self::Guide => GUIDE_SYSTEM_PROMPT,
            Self::Statusline => STATUSLINE_SYSTEM_PROMPT,
        }
    }

    /// Tool names this agent type is allowed to use.
    /// Returns `None` for General (no restrictions).
    pub fn allowed_tools(&self) -> Option<&'static [&'static str]> {
        match self {
            Self::Plan => Some(PLAN_TOOLS),
            Self::Explore => Some(EXPLORE_TOOLS),
            Self::Verify => Some(VERIFY_TOOLS),
            Self::General => None, // all tools
            Self::Guide => Some(GUIDE_TOOLS),
            Self::Statusline => Some(STATUSLINE_TOOLS),
        }
    }

    /// Suggested default model for this agent type.
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Plan | Self::Verify | Self::General => "claude-sonnet-4-20250514",
            Self::Explore | Self::Guide | Self::Statusline => "claude-haiku-4-5-20251001",
        }
    }

    /// All known agent type variants.
    pub fn all() -> &'static [AgentType] {
        &[
            Self::Plan,
            Self::Explore,
            Self::Verify,
            Self::General,
            Self::Guide,
            Self::Statusline,
        ]
    }
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Plan => "plan",
            Self::Explore => "explore",
            Self::Verify => "verify",
            Self::General => "general",
            Self::Guide => "guide",
            Self::Statusline => "statusline",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// Tool whitelists per agent type
// ---------------------------------------------------------------------------

/// Plan agent: read-only tools + write plans.
const PLAN_TOOLS: &[&str] = &[
    "read_file",
    "glob",
    "grep",
    "tool_search",
    "web_fetch",
    "web_search",
    "file_write",
    "file_edit",
    "notebook_edit",
];

/// Explore agent: read-only + search tools for codebase research.
const EXPLORE_TOOLS: &[&str] = &[
    "read_file",
    "glob",
    "grep",
    "bash",
    "tool_search",
    "web_fetch",
    "web_search",
];

/// Verify agent: read + search + test execution + write for test results.
const VERIFY_TOOLS: &[&str] = &[
    "read_file",
    "glob",
    "grep",
    "bash",
    "tool_search",
    "web_fetch",
    "web_search",
    "file_write",
    "file_edit",
];

/// Guide agent: read-only tools for help/tutorials.
const GUIDE_TOOLS: &[&str] = &[
    "read_file",
    "glob",
    "grep",
    "tool_search",
    "web_fetch",
    "web_search",
];

/// Statusline agent: read + write config files.
const STATUSLINE_TOOLS: &[&str] = &["read_file", "file_write", "file_edit"];

// ---------------------------------------------------------------------------
// System prompts per agent type
// ---------------------------------------------------------------------------

const PLAN_SYSTEM_PROMPT: &str = "\
You are a planning agent. Your job is to research the codebase and create \
detailed implementation plans. You have read-only access to the codebase \
plus the ability to write plan files.\n\n\
Focus on:\n\
- Understanding the existing code architecture\n\
- Identifying files that need modification\n\
- Creating step-by-step implementation plans\n\
- Noting potential risks and dependencies\n\n\
Do NOT implement code changes — only plan them.";

const EXPLORE_SYSTEM_PROMPT: &str = "\
You are an exploration agent specialized in codebase research. Your job is \
to quickly find files, search code for patterns, and answer questions about \
the codebase structure.\n\n\
Focus on:\n\
- Finding relevant files and code patterns\n\
- Understanding module structure and dependencies\n\
- Answering specific questions about the codebase\n\
- Providing concise, factual summaries\n\n\
Keep responses focused and concise. Report findings, not opinions.";

const VERIFY_SYSTEM_PROMPT: &str = "\
You are a verification agent for code review. Your job is to review code \
changes, run tests, and identify issues.\n\n\
Focus on:\n\
- Checking code correctness and edge cases\n\
- Running test suites and reporting results\n\
- Identifying potential bugs or regressions\n\
- Verifying compliance with project standards\n\n\
Be thorough but concise in your findings.";

const GENERAL_SYSTEM_PROMPT: &str = "\
You are a general-purpose agent. You have access to all tools and can \
handle any task including file modifications, code execution, and research.";

const GUIDE_SYSTEM_PROMPT: &str = "\
You are a guide agent providing help and tutorials. You have read-only \
access to documentation and code.\n\n\
Focus on:\n\
- Explaining code patterns and architecture\n\
- Providing usage examples and tutorials\n\
- Answering questions about features and configuration\n\n\
Be clear, helpful, and concise.";

const STATUSLINE_SYSTEM_PROMPT: &str = "\
You are a statusline configuration agent. Your job is to help configure \
terminal statusline settings by reading and writing configuration files.\n\n\
Focus on:\n\
- Reading current configuration\n\
- Making targeted config file edits\n\
- Explaining available options";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_loose_valid() {
        assert_eq!(AgentType::from_str_loose("plan"), Some(AgentType::Plan));
        assert_eq!(
            AgentType::from_str_loose("planner"),
            Some(AgentType::Plan)
        );
        assert_eq!(
            AgentType::from_str_loose("explore"),
            Some(AgentType::Explore)
        );
        assert_eq!(
            AgentType::from_str_loose("explorer"),
            Some(AgentType::Explore)
        );
        assert_eq!(
            AgentType::from_str_loose("VERIFY"),
            Some(AgentType::Verify)
        );
        assert_eq!(
            AgentType::from_str_loose("review"),
            Some(AgentType::Verify)
        );
        assert_eq!(
            AgentType::from_str_loose("General"),
            Some(AgentType::General)
        );
        assert_eq!(AgentType::from_str_loose("guide"), Some(AgentType::Guide));
        assert_eq!(
            AgentType::from_str_loose("statusline"),
            Some(AgentType::Statusline)
        );
    }

    #[test]
    fn test_from_str_loose_invalid() {
        assert_eq!(AgentType::from_str_loose("unknown"), None);
        assert_eq!(AgentType::from_str_loose(""), None);
    }

    #[test]
    fn test_system_prompts_non_empty() {
        for agent_type in AgentType::all() {
            assert!(
                !agent_type.system_prompt().is_empty(),
                "{agent_type} has empty system prompt"
            );
        }
    }

    #[test]
    fn test_general_has_no_tool_restriction() {
        assert!(AgentType::General.allowed_tools().is_none());
    }

    #[test]
    fn test_specialized_types_have_tool_restrictions() {
        for agent_type in AgentType::all() {
            if *agent_type == AgentType::General {
                continue;
            }
            let tools = agent_type
                .allowed_tools()
                .expect("non-General type should have tool whitelist");
            assert!(!tools.is_empty(), "{agent_type} has empty tool whitelist");
        }
    }

    #[test]
    fn test_plan_tools_include_read_and_write() {
        let tools = AgentType::Plan.allowed_tools().unwrap();
        assert!(tools.contains(&"read_file"));
        assert!(tools.contains(&"file_write"));
        assert!(!tools.contains(&"bash")); // Plan shouldn't run shell
    }

    #[test]
    fn test_explore_tools_include_search() {
        let tools = AgentType::Explore.allowed_tools().unwrap();
        assert!(tools.contains(&"glob"));
        assert!(tools.contains(&"grep"));
        assert!(tools.contains(&"bash"));
        assert!(!tools.contains(&"file_write")); // Explore shouldn't write
    }

    #[test]
    fn test_display_roundtrip() {
        for agent_type in AgentType::all() {
            let s = agent_type.to_string();
            assert_eq!(
                AgentType::from_str_loose(&s),
                Some(*agent_type),
                "roundtrip failed for {agent_type}"
            );
        }
    }

    #[test]
    fn test_all_returns_six_types() {
        assert_eq!(AgentType::all().len(), 6);
    }

    #[test]
    fn test_serde_roundtrip() {
        let t = AgentType::Plan;
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"plan\"");
        let parsed: AgentType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, AgentType::Plan);
    }
}
