//! Reasoning effort command: /effort — set thinking budget for the LLM.

use super::{CommandContext, CommandOutput, SlashCommand};

/// /effort <level> — set reasoning effort (low/medium/high/max).
pub struct EffortCommand;

impl SlashCommand for EffortCommand {
    fn name(&self) -> &str {
        "effort"
    }
    fn description(&self) -> &str {
        "Set reasoning effort (low/medium/high/max)"
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let level = args.trim().to_lowercase();

        if level.is_empty() {
            // Show current effort level.
            let state = ctx.state_store.current();
            let current = if state.feature_flags.is_enabled("effort_max") {
                "max"
            } else if state.feature_flags.is_enabled("effort_high") {
                "high"
            } else if state.feature_flags.is_enabled("effort_low") {
                "low"
            } else {
                "medium (default)"
            };
            return CommandOutput::Message(format!("Current reasoning effort: {current}"));
        }

        let (budget_label, flags_to_enable, flags_to_disable) = match level.as_str() {
            "low" => (
                "low",
                vec!["effort_low"],
                vec!["effort_high", "effort_max"],
            ),
            "medium" | "med" | "default" => (
                "medium",
                vec![],
                vec!["effort_low", "effort_high", "effort_max"],
            ),
            "high" | "hi" => (
                "high",
                vec!["effort_high"],
                vec!["effort_low", "effort_max"],
            ),
            "max" | "maximum" => (
                "max",
                vec!["effort_max"],
                vec!["effort_low", "effort_high"],
            ),
            _ => {
                return CommandOutput::Error(
                    "Usage: /effort <low|medium|high|max>".into(),
                );
            }
        };

        // Clear old flags, set new ones.
        for flag in flags_to_disable {
            if ctx.state_store.is_feature_enabled(flag) {
                ctx.state_store.toggle_feature(flag);
            }
        }
        for flag in flags_to_enable {
            if !ctx.state_store.is_feature_enabled(flag) {
                ctx.state_store.toggle_feature(flag);
            }
        }

        CommandOutput::Message(format!("Reasoning effort set to: {budget_label}"))
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
    fn test_effort_show_default() {
        let cmd = EffortCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("medium")),
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_effort_set_high() {
        let cmd = EffortCommand;
        let ctx = make_ctx();
        let output = cmd.execute("high", &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("high")),
            _ => panic!("Expected message"),
        }
        assert!(ctx.state_store.is_feature_enabled("effort_high"));
        assert!(!ctx.state_store.is_feature_enabled("effort_low"));
    }

    #[test]
    fn test_effort_set_low() {
        let cmd = EffortCommand;
        let ctx = make_ctx();
        let output = cmd.execute("low", &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("low")),
            _ => panic!("Expected message"),
        }
        assert!(ctx.state_store.is_feature_enabled("effort_low"));
    }

    #[test]
    fn test_effort_set_max() {
        let cmd = EffortCommand;
        let ctx = make_ctx();
        let output = cmd.execute("max", &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("max")),
            _ => panic!("Expected message"),
        }
        assert!(ctx.state_store.is_feature_enabled("effort_max"));
    }

    #[test]
    fn test_effort_set_medium_clears_flags() {
        let cmd = EffortCommand;
        let ctx = make_ctx();
        // Set to high first.
        cmd.execute("high", &ctx);
        assert!(ctx.state_store.is_feature_enabled("effort_high"));
        // Then reset to medium.
        cmd.execute("medium", &ctx);
        assert!(!ctx.state_store.is_feature_enabled("effort_high"));
        assert!(!ctx.state_store.is_feature_enabled("effort_low"));
        assert!(!ctx.state_store.is_feature_enabled("effort_max"));
    }

    #[test]
    fn test_effort_invalid() {
        let cmd = EffortCommand;
        let ctx = make_ctx();
        let output = cmd.execute("extreme", &ctx);
        match output {
            CommandOutput::Error(msg) => assert!(msg.contains("Usage")),
            _ => panic!("Expected error"),
        }
    }
}
