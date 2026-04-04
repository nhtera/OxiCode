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
        &[],
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
pub fn assemble_system_prompt_with_modes(
    global_claude_md: Option<&str>,
    project_claude_md: Option<&str>,
    skills_prompt: Option<&str>,
    memory_content: Option<&str>,
    active_skills: &[String],
) -> String {
    let mut parts = vec![BASE_SYSTEM_PROMPT.to_string()];

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

    if let Some(skills) = skills_prompt {
        parts.push(format!("\n# Active Skills\n\n{skills}"));
    }

    parts.join("\n")
}

const BASE_SYSTEM_PROMPT: &str = r"You are OxiCode, a Rust-powered CLI assistant for software engineering tasks.

You help users with:
- Writing, reviewing, and debugging code
- Navigating and understanding codebases
- Running commands and managing files
- Answering technical questions

Be concise and direct. Prefer action over explanation.";

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
        let prompt = assemble_system_prompt_with_modes(None, None, None, None, &skills);
        assert!(prompt.contains("Active Modes"));
        assert!(prompt.contains("advisor mode"));
        assert!(prompt.contains("ask the user for confirmation"));
    }

    #[test]
    fn test_sandbox_mode_injection() {
        let skills = vec!["sandbox_mode".to_string()];
        let prompt = assemble_system_prompt_with_modes(None, None, None, None, &skills);
        assert!(prompt.contains("Active Modes"));
        assert!(prompt.contains("Sandbox mode is active"));
    }

    #[test]
    fn test_both_modes_injection() {
        let skills = vec!["advisor_mode".to_string(), "sandbox_mode".to_string()];
        let prompt = assemble_system_prompt_with_modes(None, None, None, None, &skills);
        assert!(prompt.contains("advisor mode"));
        assert!(prompt.contains("Sandbox mode"));
    }

    #[test]
    fn test_no_modes_no_section() {
        let skills = vec!["extended_thinking".to_string()];
        let prompt = assemble_system_prompt_with_modes(None, None, None, None, &skills);
        assert!(!prompt.contains("Active Modes"));
    }

    #[test]
    fn test_modes_appear_before_global_instructions() {
        let skills = vec!["advisor_mode".to_string()];
        let prompt = assemble_system_prompt_with_modes(Some("global"), None, None, None, &skills);
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
}
