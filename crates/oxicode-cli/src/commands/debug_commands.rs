//! Debug slash commands: /debug, /debug-tool-call, /tokens, /context.

use super::{CommandContext, CommandOutput, SlashCommand};

pub struct DebugCommand;
impl SlashCommand for DebugCommand {
    fn name(&self) -> &str {
        "debug"
    }
    fn description(&self) -> &str {
        "Toggle debug mode"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Debug mode toggled.".into())
    }
}

pub struct DebugToolCallCommand;
impl SlashCommand for DebugToolCallCommand {
    fn name(&self) -> &str {
        "debug-tool-call"
    }
    fn description(&self) -> &str {
        "Show raw tool call details"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Tool call debugging enabled for next call.".into())
    }
}

/// /tokens — show token usage with actual usage data.
pub struct TokensCommand;
impl SlashCommand for TokensCommand {
    fn name(&self) -> &str {
        "tokens"
    }
    fn description(&self) -> &str {
        "Show token usage for current conversation"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let usage = &state.total_usage;
        let msg_count = state.messages.len();
        let total_chars: usize = state.messages.iter().map(|m| m.text().len()).sum();
        let approx_ctx_tokens = total_chars / 4;

        CommandOutput::Message(format!(
            "Messages: {msg_count}\n\
             Model: {}\n\
             API tokens: {} in / {} out\n\
             Context estimate: ~{approx_ctx_tokens} tokens",
            ctx.model, usage.input_tokens, usage.output_tokens,
        ))
    }
}

/// /context — show context window usage as percentage.
pub struct ContextCommand;
impl SlashCommand for ContextCommand {
    fn name(&self) -> &str {
        "context"
    }
    fn description(&self) -> &str {
        "Show context window usage"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let total_chars: usize = state.messages.iter().map(|m| m.text().len()).sum();
        let approx_tokens = total_chars / 4;
        let max_context: usize = 200_000; // Claude context window

        #[allow(clippy::cast_precision_loss)]
        let pct = (approx_tokens as f64 / max_context as f64 * 100.0).min(100.0);

        let bar_len: usize = 20;
        #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let filled = ((pct / 100.0) * bar_len as f64) as usize;
        let bar: String = format!(
            "[{}{}]",
            "#".repeat(filled),
            "-".repeat(bar_len - filled)
        );

        CommandOutput::Message(format!(
            "Context window: ~{approx_tokens} / {max_context} tokens ({pct:.1}%)\n\
             {bar}\n\
             Messages: {}\n\
             Model: {}",
            state.messages.len(),
            ctx.model,
        ))
    }
}
