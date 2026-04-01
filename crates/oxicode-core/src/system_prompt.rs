/// Assemble the system prompt from base instructions and optional CLAUDE.md content.
pub fn assemble_system_prompt(
    global_claude_md: Option<&str>,
    project_claude_md: Option<&str>,
) -> String {
    let mut parts = vec![BASE_SYSTEM_PROMPT.to_string()];

    if let Some(global) = global_claude_md {
        parts.push(format!("\n# User's Global Instructions\n\n{global}"));
    }

    if let Some(project) = project_claude_md {
        parts.push(format!("\n# Project Instructions\n\n{project}"));
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
        let prompt = assemble_system_prompt(None, None);
        assert!(prompt.contains("OxiCode"));
        assert!(!prompt.contains("Global Instructions"));
    }

    #[test]
    fn test_with_claude_md() {
        let prompt = assemble_system_prompt(Some("global rules"), Some("project rules"));
        assert!(prompt.contains("global rules"));
        assert!(prompt.contains("project rules"));
        assert!(prompt.contains("Global Instructions"));
        assert!(prompt.contains("Project Instructions"));
    }
}
