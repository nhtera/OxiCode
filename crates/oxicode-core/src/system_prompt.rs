/// Assemble the system prompt from base instructions, optional CLAUDE.md content,
/// active skills, and project memory.
pub fn assemble_system_prompt(
    global_claude_md: Option<&str>,
    project_claude_md: Option<&str>,
    skills_prompt: Option<&str>,
    memory_content: Option<&str>,
) -> String {
    assemble_system_prompt_with_modes(
        global_claude_md,
        project_claude_md,
        skills_prompt,
        memory_content,
        None,
        &[],
    )
}

/// Build the dynamic environment-info section.
///
/// Injects platform, OS version, shell, working directory, and git status.
pub fn build_env_info_section(working_dir: Option<&str>) -> String {
    let platform = if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };

    let os_version = {
        #[cfg(target_os = "windows")]
        {
            let ver = std::process::Command::new("cmd")
                .args(["/c", "ver"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let arch = std::env::var("PROCESSOR_ARCHITECTURE").unwrap_or_default();
            match ver {
                Some(v) => format!("{v} ({arch})"),
                None => format!("Windows ({arch})"),
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::process::Command::new("uname")
                .args(["-s", "-r"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| platform.to_string())
        }
    };

    let shell_env = std::env::var("SHELL").unwrap_or_default();
    let shell_name = if shell_env.contains("zsh") {
        "zsh"
    } else if shell_env.contains("bash") {
        "bash"
    } else if shell_env.contains("fish") {
        "fish"
    } else if cfg!(target_os = "windows") {
        "powershell"
    } else if shell_env.is_empty() {
        "unknown"
    } else {
        &shell_env
    };

    let cwd = working_dir.unwrap_or(".");
    let is_git = std::path::Path::new(cwd).join(".git").exists();

    let date = chrono::Local::now().format("%Y-%m-%d").to_string();

    format!(
        "\n<env>\nWorking directory: {cwd}\nIs git repo: {is_git}\nPlatform: {platform}\n\
         OS: {os_version}\nShell: {shell_name}\nDate: {date}\n</env>"
    )
}

/// Generate the mode-injection text for active skills.
///
/// Returns `Some(text)` if any mode is active, `None` otherwise.
/// This can be appended to an existing system prompt at per-turn time.
pub fn mode_injection_text(active_skills: &[String]) -> Option<String> {
    let mut mode_parts = Vec::new();
    if active_skills.iter().any(|s| s == "advisor_mode") {
        mode_parts.push(ADVISOR_MODE_PROMPT);
    }
    if active_skills.iter().any(|s| s == "sandbox_mode") {
        mode_parts.push(SANDBOX_MODE_PROMPT);
    }
    if mode_parts.is_empty() {
        None
    } else {
        Some(format!("\n# Active Modes\n\n{}", mode_parts.join("\n\n")))
    }
}

/// Assemble system prompt with active skill modes.
///
/// `active_skills` is the list from `AppState::active_skills`.
/// When "advisor_mode" is present, an advisor directive is injected.
/// When "sandbox_mode" is present, a sandbox notice is injected.
/// `relevant_memories` is the formatted output from `memory_selector::format_memories_for_prompt`.
pub fn assemble_system_prompt_with_modes(
    global_claude_md: Option<&str>,
    project_claude_md: Option<&str>,
    skills_prompt: Option<&str>,
    memory_content: Option<&str>,
    relevant_memories: Option<&str>,
    active_skills: &[String],
) -> String {
    let mut parts = vec![BASE_SYSTEM_PROMPT.to_string()];

    // Core capability and tool-use instruction sections.
    parts.push(CORE_CAPABILITIES.to_string());
    parts.push(TOOL_USE_GUIDELINES.to_string());
    parts.push(ACTIONS_SECTION.to_string());
    parts.push(SAFETY_GUIDELINES.to_string());

    // Inject mode-specific directives.
    let mut mode_parts = Vec::new();
    if active_skills.iter().any(|s| s == "advisor_mode") {
        mode_parts.push(ADVISOR_MODE_PROMPT);
    }
    if active_skills.iter().any(|s| s == "sandbox_mode") {
        mode_parts.push(SANDBOX_MODE_PROMPT);
    }
    if !mode_parts.is_empty() {
        parts.push(format!("\n# Active Modes\n\n{}", mode_parts.join("\n\n")));
    }

    if let Some(global) = global_claude_md {
        parts.push(format!("\n# User's Global Instructions\n\n{global}"));
    }

    if let Some(project) = project_claude_md {
        parts.push(format!("\n# Project Instructions\n\n{project}"));
    }

    if let Some(memory) = memory_content {
        if !memory.is_empty() {
            parts.push(format!("\n# Project Memory\n\n{memory}"));
        }
    }

    if let Some(relevant) = relevant_memories {
        if !relevant.is_empty() {
            parts.push(format!("\n{relevant}"));
        }
    }

    if let Some(skills) = skills_prompt {
        parts.push(format!("\n# Active Skills\n\n{skills}"));
    }

    parts.join("\n")
}

const BASE_SYSTEM_PROMPT: &str = r"You are OxiCode, a Rust-powered CLI agent for software engineering tasks.

You are an interactive agentic coding assistant. You have access to tools that let you read files, write files, execute shell commands, search the web, and more. You are operating as an autonomous agent — use your tools proactively to accomplish tasks rather than just describing what to do.

When the user asks you to do something, DO it using your tools. Don't just explain how — actually perform the action.";

const CORE_CAPABILITIES: &str = r"
## Capabilities

You have access to powerful tools for software engineering tasks:
- **Read/Write files**: Read any file, write new files, edit existing files with precise diffs
- **Execute commands**: Run bash commands, PowerShell scripts, background processes
- **Search**: Glob patterns, regex grep, web search, file content search
- **Web**: Fetch URLs, search the internet
- **Agents**: Spawn parallel sub-agents for complex multi-step work
- **Memory**: Persistent notes across sessions via the memory system
- **MCP servers**: Connect to external tools and APIs via Model Context Protocol
- **Jupyter notebooks**: Read and edit notebook cells

## How to approach tasks

1. **Understand before acting**: Read relevant files before making changes
2. **Minimal changes**: Only modify what's needed. Don't refactor unrequested code.
3. **Verify**: Check your work with tests or by reading the result
4. **Communicate blockers**: If stuck, ask the user rather than guessing
";

const TOOL_USE_GUIDELINES: &str = r"
## Tool use guidelines

- **ALWAYS use tools** to read files, run commands, and search — never guess file contents
- Use dedicated tools (file_read, file_edit, glob, grep) instead of bash equivalents
- For searches, prefer grep tool over `grep` command; prefer glob tool over `find` command
- Parallelize independent tool calls in a single response when possible
- For file edits: always read the file first, then make targeted edits
- Bash commands timeout after 2 minutes; use background mode for long operations
";

const ACTIONS_SECTION: &str = r"
## Executing actions with care

Carefully consider the reversibility and blast radius of actions. For actions
that are hard to reverse, affect shared systems, or could be risky or
destructive, check with the user before proceeding. Authorization stands for
the scope specified, not beyond. Match the scope of your actions to what was
actually requested.
";

const SAFETY_GUIDELINES: &str = r"
## Safety guidelines

- Never delete files without explicit user confirmation
- Don't modify protected config files (.gitconfig, .bashrc, .zshrc) unless asked
- Be careful with destructive operations (rm -rf, DROP TABLE, etc.)
- Don't commit secrets, credentials, or API keys
- For ambiguous destructive actions, ask before proceeding
";

const ADVISOR_MODE_PROMPT: &str = "\
You are in **advisor mode**. In this mode:
- Suggest approaches and explain your reasoning, but do NOT execute tools directly.
- Always ask the user for confirmation before taking any action.
- Present options and trade-offs rather than making unilateral decisions.
- If the user asks you to do something, describe what you would do and ask \"Shall I proceed?\"";

const SANDBOX_MODE_PROMPT: &str = "\
**Sandbox mode is active.** Shell execution tools (bash, powershell, repl) are \
disabled for this session. You may still read/write files and use other tools.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_prompt_only() {
        let prompt = assemble_system_prompt(None, None, None, None);
        assert!(prompt.contains("OxiCode"));
        assert!(prompt.contains("Capabilities"));
        assert!(prompt.contains("Tool use guidelines"));
        assert!(prompt.contains("Safety guidelines"));
        assert!(!prompt.contains("Global Instructions"));
    }

    #[test]
    fn test_with_claude_md() {
        let prompt =
            assemble_system_prompt(Some("global rules"), Some("project rules"), None, None);
        assert!(prompt.contains("global rules"));
        assert!(prompt.contains("project rules"));
        assert!(prompt.contains("Global Instructions"));
        assert!(prompt.contains("Project Instructions"));
    }

    #[test]
    fn test_with_skills_prompt() {
        let prompt = assemble_system_prompt(None, None, Some("use sequential-thinking"), None);
        assert!(prompt.contains("Active Skills"));
        assert!(prompt.contains("use sequential-thinking"));
    }

    #[test]
    fn test_all_sections() {
        let prompt = assemble_system_prompt(
            Some("global rules"),
            Some("project rules"),
            Some("skill a"),
            Some("memory content"),
        );
        assert!(prompt.contains("Global Instructions"));
        assert!(prompt.contains("Project Instructions"));
        assert!(prompt.contains("Active Skills"));
        assert!(prompt.contains("Project Memory"));
        assert!(prompt.contains("memory content"));
    }

    #[test]
    fn test_with_memory_only() {
        let prompt = assemble_system_prompt(None, None, None, Some("Use snake_case"));
        assert!(prompt.contains("Project Memory"));
        assert!(prompt.contains("Use snake_case"));
    }

    #[test]
    fn test_empty_memory_skipped() {
        let prompt = assemble_system_prompt(None, None, None, Some(""));
        assert!(!prompt.contains("Project Memory"));
    }

    #[test]
    fn test_section_ordering() {
        let prompt = assemble_system_prompt(
            Some("global"),
            Some("project"),
            Some("skills"),
            Some("memory"),
        );
        // Memory should come before Skills.
        let mem_pos = prompt.find("Project Memory").unwrap();
        let skill_pos = prompt.find("Active Skills").unwrap();
        assert!(mem_pos < skill_pos);
    }

    // -- Mode injection tests --

    #[test]
    fn test_advisor_mode_injection() {
        let skills = vec!["advisor_mode".to_string()];
        let prompt = assemble_system_prompt_with_modes(None, None, None, None, None, &skills);
        assert!(prompt.contains("Active Modes"));
        assert!(prompt.contains("advisor mode"));
        assert!(prompt.contains("ask the user for confirmation"));
    }

    #[test]
    fn test_sandbox_mode_injection() {
        let skills = vec!["sandbox_mode".to_string()];
        let prompt = assemble_system_prompt_with_modes(None, None, None, None, None, &skills);
        assert!(prompt.contains("Active Modes"));
        assert!(prompt.contains("Sandbox mode is active"));
    }

    #[test]
    fn test_both_modes_injection() {
        let skills = vec!["advisor_mode".to_string(), "sandbox_mode".to_string()];
        let prompt = assemble_system_prompt_with_modes(None, None, None, None, None, &skills);
        assert!(prompt.contains("advisor mode"));
        assert!(prompt.contains("Sandbox mode"));
    }

    #[test]
    fn test_no_modes_no_section() {
        let skills = vec!["extended_thinking".to_string()];
        let prompt = assemble_system_prompt_with_modes(None, None, None, None, None, &skills);
        assert!(!prompt.contains("Active Modes"));
    }

    #[test]
    fn test_modes_appear_before_global_instructions() {
        let skills = vec!["advisor_mode".to_string()];
        let prompt =
            assemble_system_prompt_with_modes(Some("global"), None, None, None, None, &skills);
        let modes_pos = prompt.find("Active Modes").unwrap();
        let global_pos = prompt.find("Global Instructions").unwrap();
        assert!(
            modes_pos < global_pos,
            "Modes should come before global instructions"
        );
    }

    // -- mode_injection_text tests --

    #[test]
    fn test_mode_injection_none_when_no_modes() {
        let skills = vec!["extended_thinking".to_string()];
        assert!(mode_injection_text(&skills).is_none());
    }

    #[test]
    fn test_mode_injection_advisor() {
        let skills = vec!["advisor_mode".to_string()];
        let text = mode_injection_text(&skills).unwrap();
        assert!(text.contains("advisor mode"));
        assert!(text.contains("Active Modes"));
    }

    #[test]
    fn test_mode_injection_sandbox() {
        let skills = vec!["sandbox_mode".to_string()];
        let text = mode_injection_text(&skills).unwrap();
        assert!(text.contains("Sandbox mode"));
    }

    // -- Relevant memories tests --

    #[test]
    fn test_relevant_memories_injected() {
        let prompt = assemble_system_prompt_with_modes(
            None,
            None,
            None,
            Some("MEMORY.md content"),
            Some("## Relevant Memories\n\n- Use Rust for CLI\n- Prefer snake_case\n"),
            &[],
        );
        assert!(prompt.contains("Project Memory"));
        assert!(prompt.contains("Relevant Memories"));
        assert!(prompt.contains("Use Rust for CLI"));
    }

    #[test]
    fn test_relevant_memories_after_project_memory() {
        let prompt = assemble_system_prompt_with_modes(
            None,
            None,
            None,
            Some("memory"),
            Some("## Relevant Memories\n\n- fact"),
            &[],
        );
        let mem_pos = prompt.find("Project Memory").unwrap();
        let rel_pos = prompt.find("Relevant Memories").unwrap();
        assert!(rel_pos > mem_pos);
    }

    #[test]
    fn test_empty_relevant_memories_skipped() {
        let prompt = assemble_system_prompt_with_modes(None, None, None, None, Some(""), &[]);
        assert!(!prompt.contains("Relevant Memories"));
    }
}
