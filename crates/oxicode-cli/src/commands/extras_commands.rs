//! Extra commands: /voice, /desktop, /mobile, /bridge.
//!
//! These are low-priority commands that provide stubs or feature-gated functionality.

use super::{CommandContext, CommandOutput, SlashCommand};

/// /voice — voice input via Whisper API (feature-gated).
pub struct VoiceCommand;

impl SlashCommand for VoiceCommand {
    fn name(&self) -> &str {
        "voice"
    }
    fn description(&self) -> &str {
        "Voice input (requires 'voice' feature)"
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        #[cfg(feature = "voice")]
        {
            match args.trim() {
                "on" | "start" => CommandOutput::Message(
                    "Voice input enabled. Speak into your microphone…\n\
                     (Whisper API integration placeholder — requires OPENAI_API_KEY)"
                        .to_string(),
                ),
                "off" | "stop" => {
                    CommandOutput::Message("Voice input disabled.".to_string())
                }
                "" => CommandOutput::Message(
                    "Voice input: use /voice on to start, /voice off to stop.\n\
                     Requires OPENAI_API_KEY for Whisper transcription."
                        .to_string(),
                ),
                _ => CommandOutput::Error(
                    "Usage: /voice [on|off]".to_string(),
                ),
            }
        }
        #[cfg(not(feature = "voice"))]
        {
            let _ = args; // suppress unused warning
            CommandOutput::Message(
                "Voice input is not available. Compile with: cargo build --features voice"
                    .to_string(),
            )
        }
    }
}

/// /desktop — open in native desktop app (placeholder).
pub struct DesktopCommand;

impl SlashCommand for DesktopCommand {
    fn name(&self) -> &str {
        "desktop"
    }
    fn description(&self) -> &str {
        "Open in desktop app (if available)"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        // Detect platform and try to launch (use status() to reap child process).
        #[cfg(target_os = "macos")]
        {
            if std::process::Command::new("open")
                .arg("-a")
                .arg("OxiCode")
                .status()
                .is_ok()
            {
                return CommandOutput::Message("Opening OxiCode desktop app…".to_string());
            }
        }
        #[cfg(target_os = "linux")]
        {
            if std::process::Command::new("xdg-open")
                .arg("oxicode://")
                .status()
                .is_ok()
            {
                return CommandOutput::Message("Opening OxiCode desktop app…".to_string());
            }
        }
        #[cfg(target_os = "windows")]
        {
            if std::process::Command::new("cmd")
                .args(["/C", "start", "oxicode://"])
                .status()
                .is_ok()
            {
                return CommandOutput::Message("Opening OxiCode desktop app…".to_string());
            }
        }

        CommandOutput::Message(
            "OxiCode desktop app not found.\n\
             Visit https://github.com/nicktien007/oxicode/releases for downloads."
                .to_string(),
        )
    }
}

/// /mobile — show QR code for mobile companion (placeholder).
pub struct MobileCommand;

impl SlashCommand for MobileCommand {
    fn name(&self) -> &str {
        "mobile"
    }
    fn description(&self) -> &str {
        "Show QR code for mobile companion"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(
            "Mobile companion not yet available.\n\
             Future: scan a QR code to connect from your phone.\n\
             Track progress at: https://github.com/nicktien007/oxicode/issues"
                .to_string(),
        )
    }
}

/// /bridge — start bridge mode for remote access (placeholder).
pub struct BridgeCommand;

impl SlashCommand for BridgeCommand {
    fn name(&self) -> &str {
        "bridge"
    }
    fn description(&self) -> &str {
        "Bridge mode for remote/cloud deployment"
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let port = args.trim().parse::<u16>().unwrap_or(8080);
        CommandOutput::Message(format!(
            "Bridge mode placeholder.\n\
             Future: oxicode --bridge --port {port}\n\
             Will support:\n\
             - Headless long-running server\n\
             - Multi-session management\n\
             - JWT authentication\n\
             - Capacity limits"
        ))
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
    fn test_voice_command_no_feature() {
        let cmd = VoiceCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                // With or without voice feature, should produce a message.
                assert!(!msg.is_empty());
            }
            _ => panic!("Expected message output"),
        }
    }

    #[test]
    fn test_desktop_command() {
        let cmd = DesktopCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(!msg.is_empty()),
            _ => panic!("Expected message output"),
        }
    }

    #[test]
    fn test_mobile_command() {
        let cmd = MobileCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("Mobile")),
            _ => panic!("Expected message output"),
        }
    }

    #[test]
    fn test_bridge_command_default_port() {
        let cmd = BridgeCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("8080")),
            _ => panic!("Expected message output"),
        }
    }

    #[test]
    fn test_bridge_command_custom_port() {
        let cmd = BridgeCommand;
        let ctx = make_ctx();
        let output = cmd.execute("3000", &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("3000")),
            _ => panic!("Expected message output"),
        }
    }
}
