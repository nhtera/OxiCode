//! `/good-claude [message]` — record positive feedback for the current turn.
//!
//! Stores a feedback token in `active_skills` of the form
//! `"feedback:positive:<turn_count>"` so the engine can surface it in
//! analytics and fine-tuning pipelines.

use super::{CommandContext, CommandOutput, SlashCommand};

/// `/good-claude [message]` — mark the last turn as positive feedback.
pub struct GoodClaudeCommand;

impl SlashCommand for GoodClaudeCommand {
    fn name(&self) -> &str {
        "good-claude"
    }

    fn description(&self) -> &str {
        "Mark the last assistant turn as positive feedback"
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let turn_count = ctx.state_store.current().messages.len();
        let key = format!("feedback:positive:{turn_count}");

        // Avoid duplicates for the same turn.
        let already_recorded = ctx
            .state_store
            .current()
            .active_skills
            .iter()
            .any(|s| s == &key);

        if already_recorded {
            return CommandOutput::Message(
                "Positive feedback already recorded for this turn.\n\
                 Thanks for the kind words!"
                    .to_string(),
            );
        }

        ctx.state_store.update(|s| {
            s.active_skills.push(key.clone());
        });

        let note = args.trim();
        let confirmation = if note.is_empty() {
            format!(
                "Positive feedback recorded for turn {turn_count}.\n\
                 Thanks — this helps improve future responses!"
            )
        } else {
            format!(
                "Positive feedback recorded for turn {turn_count}: \"{note}\"\n\
                 Thanks — this helps improve future responses!"
            )
        };

        tracing::debug!(%key, note = %note, "Positive feedback recorded");
        CommandOutput::Message(confirmation)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
    fn test_records_feedback_key() {
        let cmd = GoodClaudeCommand;
        let ctx = make_ctx();
        cmd.execute("", &ctx);
        let state = ctx.state_store.current();
        let has_key = state
            .active_skills
            .iter()
            .any(|s| s.starts_with("feedback:positive:"));
        assert!(has_key);
    }

    #[test]
    fn test_with_message_note() {
        let cmd = GoodClaudeCommand;
        let ctx = make_ctx();
        match cmd.execute("Great explanation!", &ctx) {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("Great explanation!"));
                assert!(msg.contains("recorded"));
            }
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn test_duplicate_feedback_same_turn() {
        let cmd = GoodClaudeCommand;
        let ctx = make_ctx();
        cmd.execute("", &ctx);
        // Second invocation at same turn count.
        match cmd.execute("", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("already recorded")),
            _ => panic!("expected Message"),
        }
    }
}
