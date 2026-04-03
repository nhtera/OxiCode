//! Clipboard command: /copy — copy last assistant response to system clipboard.

use super::{CommandContext, CommandOutput, SlashCommand};

/// /copy — copy the last assistant message to the system clipboard.
pub struct CopyCommand;

impl SlashCommand for CopyCommand {
    fn name(&self) -> &str {
        "copy"
    }
    fn description(&self) -> &str {
        "Copy last response to clipboard"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();

        // Find the last assistant message.
        let last_assistant = state
            .messages
            .iter()
            .rev()
            .find(|m| m.role == oxicode_common::Role::Assistant);

        let Some(msg) = last_assistant else {
            return CommandOutput::Error("No assistant response to copy.".into());
        };

        // Extract text from content blocks.
        let text: String = msg
            .content
            .iter()
            .filter_map(|block| {
                if let oxicode_common::ContentBlock::Text { text } = block {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        if text.is_empty() {
            return CommandOutput::Error("Last assistant response is empty.".into());
        }

        match copy_to_clipboard(&text) {
            Ok(()) => CommandOutput::Message(format!(
                "Copied {} chars to clipboard.",
                text.len()
            )),
            Err(e) => CommandOutput::Error(format!("Clipboard not available: {e}")),
        }
    }
}

/// Copy text to system clipboard (platform-specific).
fn copy_to_clipboard(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("pbcopy")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to run pbcopy: {e}"))?;
        if let Some(ref mut stdin) = child.stdin {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| format!("Failed to write to pbcopy: {e}"))?;
        }
        child
            .wait()
            .map_err(|e| format!("pbcopy failed: {e}"))?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        use std::process::{Command, Stdio};
        // Try xclip first, then xsel, then wl-copy (Wayland).
        let cmds = ["xclip -selection clipboard", "xsel --clipboard --input", "wl-copy"];
        for cmd_str in &cmds {
            let parts: Vec<&str> = cmd_str.split_whitespace().collect();
            if let Ok(mut child) = Command::new(parts[0])
                .args(&parts[1..])
                .stdin(Stdio::piped())
                .spawn()
            {
                if let Some(ref mut stdin) = child.stdin {
                    let _ = stdin.write_all(text.as_bytes());
                }
                if child.wait().is_ok() {
                    return Ok(());
                }
            }
        }
        Err("No clipboard tool found (install xclip, xsel, or wl-copy)".into())
    }

    #[cfg(target_os = "windows")]
    {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("clip")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to run clip: {e}"))?;
        if let Some(ref mut stdin) = child.stdin {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| format!("Failed to write to clip: {e}"))?;
        }
        child.wait().map_err(|e| format!("clip failed: {e}"))?;
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = text;
        Err("Clipboard not supported on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_ctx() -> CommandContext {
        CommandContext {
            state_store: Arc::new(oxicode_state::StateStore::default()),
            model: "test".to_string(),
            provider_name: "test".to_string(),
            session_id: "test".to_string(),
        }
    }

    #[test]
    fn test_copy_no_messages() {
        let cmd = CopyCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Error(msg) => assert!(msg.contains("No assistant")),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn test_copy_name_and_description() {
        let cmd = CopyCommand;
        assert_eq!(cmd.name(), "copy");
        assert!(!cmd.description().is_empty());
    }
}
