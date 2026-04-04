/// Assemble the system prompt from base instructions, optional CLAUDE.md content,
/// active skills, and project memory.
pub fn assemble_system_prompt(
    global_claude_md: Option<&str>,
    project_claude_md: Option<&str>,
    skills_prompt: Option<&str>,
    memory_content: Option<&str>,
) -> String {
    let mut parts = vec![BASE_SYSTEM_PROMPT.to_string()];

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
        let prompt =
            assemble_system_prompt(None, None, Some("use sequential-thinking"), None);
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
}
