//! Ghost text completion engine for the input prompt.
//!
//! Provides dimmed autocomplete suggestions (ghost text) that appear after
//! the cursor. Type `/mo` → shows `del` as dim text → Tab/Right accepts.

use crate::widgets::command_autocomplete::SlashCommandMeta;

/// Compute ghost text completion for the current input.
///
/// Returns the **suffix** to append (not the full command). For example,
/// if input is `/mo`, returns `Some("del")` (completing to `/model`).
/// If input is `/model cl`, returns `Some("aude-sonnet-4-20250514")`.
///
/// Returns `None` when no match or input is empty.
pub fn complete(input: &str, commands: &[SlashCommandMeta]) -> Option<String> {
    let trimmed = input.trim_end();
    if trimmed.is_empty() {
        return None;
    }

    // Slash command name completion: input starts with '/' and has no space.
    if trimmed.starts_with('/') && !trimmed.contains(' ') {
        let lower = trimmed.to_lowercase();
        for cmd in commands {
            let full = format!("/{}", cmd.name);
            if full.to_lowercase().starts_with(&lower) && full.len() > lower.len() {
                return Some(full[lower.len()..].to_string());
            }
        }
    }

    // Argument completion: input starts with '/' and has a space.
    if trimmed.starts_with('/') && trimmed.contains(' ') {
        let without_slash = &trimmed[1..];
        if let Some((cmd_name, partial_arg)) = without_slash.split_once(' ') {
            let partial = partial_arg.trim();
            if !partial.is_empty() {
                // Find the command and check its arg_candidates.
                for cmd in commands {
                    if cmd.name == cmd_name {
                        for candidate in &cmd.arg_candidates {
                            if candidate.starts_with(partial) && candidate.len() > partial.len() {
                                return Some(candidate[partial.len()..].to_string());
                            }
                        }
                        break;
                    }
                }
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
            SlashCommandMeta {
                name: "clear".into(),
                description: "Clear conversation".into(),
                category: "Session".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "compact".into(),
                description: "Compact context".into(),
                category: "Session".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "help".into(),
                description: "Show help".into(),
                category: "Session".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "model".into(),
                description: "Switch model".into(),
                category: "Model".into(),
                arg_candidates: vec![
                    "claude-sonnet-4-20250514".into(),
                    "claude-opus-4-20250514".into(),
                    "claude-haiku-4-5-20251001".into(),
                ],
            },
            SlashCommandMeta {
                name: "session".into(),
                description: "Session management".into(),
                category: "Session".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "vim".into(),
                description: "Toggle vim mode".into(),
                category: "Display".into(),
                arg_candidates: vec![],
            },
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
    fn command_with_args_completes_argument() {
        let cmds = sample_commands();
        // Arg completion for /model with a matching prefix.
        assert_eq!(
            complete("/model cl", &cmds),
            Some("aude-sonnet-4-20250514".to_string())
        );
        assert_eq!(
            complete("/model claude-o", &cmds),
            Some("pus-4-20250514".to_string())
        );
    }

    #[test]
    fn command_with_unknown_args_no_ghost() {
        let cmds = sample_commands();
        // No matching argument candidate.
        assert_eq!(complete("/model xyz", &cmds), None);
        // Command without arg_candidates.
        assert_eq!(complete("/clear foo", &cmds), None);
    }

    #[test]
    fn empty_command_list() {
        assert_eq!(complete("/he", &[]), None);
    }
}
