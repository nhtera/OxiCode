//! Ghost text completion engine for the input prompt.
//!
//! Provides dimmed autocomplete suggestions (ghost text) that appear after
//! the cursor. Type `/mo` → shows `del` as dim text → Tab/Right accepts.

use crate::widgets::command_autocomplete::SlashCommandMeta;

/// Compute ghost text completion for the current input.
///
/// Returns the **suffix** to append (not the full command). For example,
/// if input is `/mo`, returns `Some("del")` (completing to `/model`).
///
/// Returns `None` when no match or input is empty.
pub fn complete(input: &str, commands: &[SlashCommandMeta]) -> Option<String> {
    let trimmed = input.trim_end();
    if trimmed.is_empty() {
        return None;
    }

    // Slash command completion: input must start with '/' and be a single token.
    if trimmed.starts_with('/') && !trimmed.contains(' ') {
        let lower = trimmed.to_lowercase();
        // Find first command that matches the prefix.
        for cmd in commands {
            let full = format!("/{}", cmd.name);
            if full.to_lowercase().starts_with(&lower) && full.len() > lower.len() {
                return Some(full[lower.len()..].to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_commands() -> Vec<SlashCommandMeta> {
        vec![
            SlashCommandMeta { name: "clear".into(), description: "Clear conversation".into() },
            SlashCommandMeta { name: "compact".into(), description: "Compact context".into() },
            SlashCommandMeta { name: "help".into(), description: "Show help".into() },
            SlashCommandMeta { name: "model".into(), description: "Switch model".into() },
            SlashCommandMeta { name: "session".into(), description: "Session management".into() },
            SlashCommandMeta { name: "vim".into(), description: "Toggle vim mode".into() },
        ]
    }

    #[test]
    fn slash_command_match() {
        let cmds = sample_commands();
        assert_eq!(complete("/mo", &cmds), Some("del".to_string()));
        assert_eq!(complete("/he", &cmds), Some("lp".to_string()));
        assert_eq!(complete("/cl", &cmds), Some("ear".to_string()));
    }

    #[test]
    fn full_command_no_ghost() {
        let cmds = sample_commands();
        assert_eq!(complete("/model", &cmds), None);
        assert_eq!(complete("/help", &cmds), None);
    }

    #[test]
    fn no_match_returns_none() {
        let cmds = sample_commands();
        assert_eq!(complete("/xyz", &cmds), None);
        assert_eq!(complete("hello", &cmds), None);
    }

    #[test]
    fn empty_input_returns_none() {
        let cmds = sample_commands();
        assert_eq!(complete("", &cmds), None);
        assert_eq!(complete("  ", &cmds), None);
    }

    #[test]
    fn command_with_args_no_ghost() {
        let cmds = sample_commands();
        // Once a space is present, don't suggest command completions.
        assert_eq!(complete("/model son", &cmds), None);
    }

    #[test]
    fn empty_command_list() {
        assert_eq!(complete("/he", &[]), None);
    }
}
