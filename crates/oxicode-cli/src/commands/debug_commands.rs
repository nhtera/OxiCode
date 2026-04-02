//! Debug slash commands: /debug, /debug-tool-call, /tokens, /context.

use super::{CommandContext, CommandOutput, SlashCommand};

pub struct DebugCommand;
impl SlashCommand for DebugCommand {
    fn name(&self) -> &str { "debug" }
    fn description(&self) -> &str { "Toggle debug mode" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Debug mode toggled.".into())
    }
}

pub struct DebugToolCallCommand;
impl SlashCommand for DebugToolCallCommand {
    fn name(&self) -> &str { "debug-tool-call" }
    fn description(&self) -> &str { "Show raw tool call details" }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message("Tool call debugging enabled for next call.".into())
    }
}

pub struct TokensCommand;
impl SlashCommand for TokensCommand {
    fn name(&self) -> &str { "tokens" }
    fn description(&self) -> &str { "Show token usage for current conversation" }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let msg_count = state.messages.len();
        CommandOutput::Message(format!(
            "Messages: {msg_count}\nModel: {}\nEstimated tokens: ~{}",
            ctx.model,
            msg_count * 500, // rough estimate
        ))
    }
}

pub struct ContextCommand;
impl SlashCommand for ContextCommand {
    fn name(&self) -> &str { "context" }
    fn description(&self) -> &str { "Show context window usage" }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        CommandOutput::Message(format!(
            "Context: {} messages, model: {}",
            state.messages.len(),
            ctx.model,
        ))
    }
}
