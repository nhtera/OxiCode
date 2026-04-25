//! Session extra commands: /tag, /btw, /thinkback, /release-notes.

use super::{CommandContext, CommandOutput, SlashCommand};

/// /tag <label> — add tag to current session metadata.
pub struct TagCommand;
impl SlashCommand for TagCommand {
    fn name(&self) -> &str {
        "tag"
    }
    fn description(&self) -> &str {
        "Tag current session with a label"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let label = args.trim();
        if label.is_empty() {
            // Show existing tags.
            let state = ctx.state_store.current();
            let tags: Vec<&str> = state
                .active_skills
                .iter()
                .filter_map(|s| s.strip_prefix("tag:"))
                .collect();
            return if tags.is_empty() {
                CommandOutput::Message("No tags. Usage: /tag <label>".into())
            } else {
                CommandOutput::Message(format!("Tags: {}", tags.join(", ")))
            };
        }

        // Sanitize: max 32 chars, alphanumeric + dash + underscore.
        let sanitized: String = label
            .chars()
            .take(32)
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();

        if sanitized.is_empty() {
            return CommandOutput::Error("Tag must contain alphanumeric characters.".into());
        }

        ctx.state_store.update(|s| {
            let tag = format!("tag:{sanitized}");
            if !s.active_skills.contains(&tag) {
                s.active_skills.push(tag);
            }
        });
        CommandOutput::Message(format!("Tagged session: {sanitized}"))
    }
}

/// /btw <message> — inject aside text without interrupting flow.
pub struct BtwCommand;
impl SlashCommand for BtwCommand {
    fn name(&self) -> &str {
        "btw"
    }
    fn description(&self) -> &str {
        "Inject an aside message into the conversation"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let msg = args.trim();
        if msg.is_empty() {
            return CommandOutput::Error("Usage: /btw <message>".into());
        }
        // Format as a bracketed aside.
        CommandOutput::Message(format!("[btw: {msg}]"))
    }
}

/// /thinkback — replay the last thinking block.
pub struct ThinkbackCommand;
impl SlashCommand for ThinkbackCommand {
    fn name(&self) -> &str {
        "thinkback"
    }
    fn description(&self) -> &str {
        "Replay last thinking block"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();

        // Find the last assistant message with a Thinking block.
        for msg in state.messages.iter().rev() {
            if msg.role != oxicode_common::Role::Assistant {
                continue;
            }
            for block in &msg.content {
                if let oxicode_common::ContentBlock::Thinking { thinking } = block {
                    let preview: String = thinking.chars().take(2000).collect();
                    let suffix = if thinking.chars().count() > 2000 {
                        "\n\n... (truncated)"
                    } else {
                        ""
                    };
                    return CommandOutput::Message(format!(
                        "Last thinking block:\n\n{preview}{suffix}"
                    ));
                }
            }
        }
        CommandOutput::Message("No thinking blocks found in this session.".into())
    }
}

/// /release-notes — show version changelog.
/// Unregistered in Phase 6 prune (2026-04-26); kept for re-introduction.
#[allow(dead_code)]
pub struct ReleaseNotesCommand;
impl SlashCommand for ReleaseNotesCommand {
    fn name(&self) -> &str {
        "release-notes"
    }
    fn description(&self) -> &str {
        "Show version changelog"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(
            "OxiCode Release Notes\n\
             =====================\n\
             \n\
             v0.1.0 (current)\n\
             - Initial Rust port from OpenClaude (TypeScript)\n\
             - 45 tools, 110+ commands, 537+ tests\n\
             - Multi-provider support (Anthropic, OpenAI, Ollama, etc.)\n\
             - TUI with vim mode, split pane, search\n\
             - MCP server integration\n\
             - Plugin marketplace\n\
             - OAuth + API key authentication\n\
             - Rate limiting with exponential backoff\n\
             \n\
             See CHANGELOG.md for full history."
                .into(),
        )
    }
}
