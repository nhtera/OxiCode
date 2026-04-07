//! Ghost text completion engine for the input prompt.
//!
//! Provides dimmed autocomplete suggestions (ghost text) that appear after
//! the cursor. Type `/mo` → shows `del` as dim text → Tab/Right accepts.

/// Known slash commands for ghost text completion.
const SLASH_COMMANDS: &[&str] = &[
    "/clear",
    "/compact",
    "/help",
    "/model",
    "/session",
    "/vim",
];

/// Compute ghost text completion for the current input.
///
/// Returns the **suffix** to append (not the full command). For example,
/// if input is `/mo`, returns `Some("del")` (completing to `/model`).
///
/// Returns `None` when no match or input is empty.
pub fn complete(input: &str) -> Option<String> {
    let trimmed = input.trim_end();
    if trimmed.is_empty() {
        return None;
    }

    // Slash command completion: input must start with '/' and be a single token.
    if trimmed.starts_with('/') && !trimmed.contains(' ') {
        let lower = trimmed.to_lowercase();
        for cmd in SLASH_COMMANDS {
            if cmd.starts_with(&lower) && cmd.len() > lower.len() {
                return Some(cmd[lower.len()..].to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_command_match() {
        assert_eq!(complete("/mo"), Some("del".to_string()));
        assert_eq!(complete("/he"), Some("lp".to_string()));
        assert_eq!(complete("/cl"), Some("ear".to_string()));
    }

    #[test]
    fn full_command_no_ghost() {
        assert_eq!(complete("/model"), None);
        assert_eq!(complete("/help"), None);
    }

    #[test]
    fn no_match_returns_none() {
        assert_eq!(complete("/xyz"), None);
        assert_eq!(complete("hello"), None);
    }

    #[test]
    fn empty_input_returns_none() {
        assert_eq!(complete(""), None);
        assert_eq!(complete("  "), None);
    }

    #[test]
    fn command_with_args_no_ghost() {
        // Once a space is present, don't suggest command completions.
        assert_eq!(complete("/model son"), None);
    }
}
