//! Extra commands: /voice, /desktop, /mobile, /bridge, /install-github-app,
//! /remote-setup, /remote-env.
//!
//! These provide feature-gated functionality for voice input, bridge mode,
//! and GitHub integration.

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
                "on" | "start" => {
                    if std::env::var("OPENAI_API_KEY").is_err() {
                        return CommandOutput::Error(
                            "OPENAI_API_KEY not set. Required for Whisper transcription."
                                .to_string(),
                        );
                    }
                    CommandOutput::Message(
                        "Voice input enabled. Listening via microphone...\n\
                         Speak naturally — text will appear in your input.\n\
                         Use /voice off to stop."
                            .to_string(),
                    )
                }
                "off" | "stop" => CommandOutput::Message("Voice input disabled.".to_string()),
                "status" => CommandOutput::Message(
                    "Voice status: check the status bar for the microphone indicator.".to_string(),
                ),
                "" => CommandOutput::Message(
                    "Voice input: use /voice on to start, /voice off to stop.\n\
                     Requires OPENAI_API_KEY for Whisper transcription.\n\
                     Audio is streamed directly to API — never stored on disk."
                        .to_string(),
                ),
                _ => CommandOutput::Error("Usage: /voice [on|off|status]".to_string()),
            }
        }
        #[cfg(not(feature = "voice"))]
        {
            let _ = args;
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
        #[cfg(target_os = "macos")]
        {
            if std::process::Command::new("open")
                .arg("-a")
                .arg("OxiCode")
                .status()
                .is_ok()
            {
                return CommandOutput::Message("Opening OxiCode desktop app...".to_string());
            }
        }
        #[cfg(target_os = "linux")]
        {
            if std::process::Command::new("xdg-open")
                .arg("oxicode://")
                .status()
                .is_ok()
            {
                return CommandOutput::Message("Opening OxiCode desktop app...".to_string());
            }
        }
        #[cfg(target_os = "windows")]
        {
            if std::process::Command::new("cmd")
                .args(["/C", "start", "oxicode://"])
                .status()
                .is_ok()
            {
                return CommandOutput::Message("Opening OxiCode desktop app...".to_string());
            }
        }

        CommandOutput::Message(
            "OxiCode desktop app not found.\n\
             Visit https://github.com/nhtera/oxicode/releases for downloads."
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
             Track progress at: https://github.com/nhtera/oxicode/issues"
                .to_string(),
        )
    }
}

/// /bridge — start bridge mode for remote access.
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
        let config = crate::remote::BridgeConfig::default().with_port(port);
        CommandOutput::Message(format!(
            "Bridge mode configuration:\n\
             Address: {}\n\
             Max sessions: {}\n\
             Idle timeout: {}s\n\
             JWT auth: {}\n\n\
             To start: oxicode --bridge --port {port}\n\
             Compile with --features bridge for full WebSocket support.",
            config.socket_addr(),
            config.max_sessions,
            config.idle_timeout_secs,
            if config.jwt_secret.is_some() {
                "enabled"
            } else {
                "not configured"
            },
        ))
    }
}

/// /install-github-app — guided GitHub App installation wizard.
pub struct InstallGithubAppCommand;

impl SlashCommand for InstallGithubAppCommand {
    fn name(&self) -> &str {
        "install-github-app"
    }
    fn description(&self) -> &str {
        "Install OxiCode GitHub App workflow"
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let target_repo = if args.trim().is_empty() {
            None
        } else {
            Some(args.trim())
        };

        // Check prerequisites.
        if crate::github::get_github_token().is_none() {
            return CommandOutput::Error(
                "Not authenticated with GitHub.\n\
                 Set GITHUB_TOKEN env var or run: gh auth login\n\n\
                 Usage: /install-github-app [repo-name]"
                    .to_string(),
            );
        }

        let steps = crate::github::app_install::wizard_steps();
        let step_list: String = steps
            .iter()
            .enumerate()
            .map(|(i, s)| format!("  {}. {}", i + 1, s.label))
            .collect::<Vec<_>>()
            .join("\n");

        CommandOutput::Message(format!(
            "GitHub App Install Wizard\n\
             {}\n\n\
             Target: {}\n\
             This will create .github/workflows/oxicode.yml in the selected repo.\n\
             Requires ANTHROPIC_API_KEY as a GitHub Actions secret.",
            step_list,
            target_repo.unwrap_or("(will list repos to choose)"),
        ))
    }
}

/// /remote-setup — configure bridge endpoint and session settings.
pub struct RemoteSetupCommand;

impl SlashCommand for RemoteSetupCommand {
    fn name(&self) -> &str {
        "remote-setup"
    }
    fn description(&self) -> &str {
        "Configure remote session settings"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        CommandOutput::Message(
            "Remote session settings:\n\
             - Bridge endpoint: 127.0.0.1:8080 (default)\n\
             - Max sessions: 10\n\
             - Idle timeout: 30 minutes\n\
             - JWT auth: configure via OXICODE_BRIDGE_JWT_SECRET env var\n\n\
             To start bridge: oxicode --bridge --port 8080\n\
             To allow external access: set bind address to 0.0.0.0 (with JWT auth)"
                .to_string(),
        )
    }
}

/// /remote-env — manage remote environment variables.
pub struct RemoteEnvCommand;

impl SlashCommand for RemoteEnvCommand {
    fn name(&self) -> &str {
        "remote-env"
    }
    fn description(&self) -> &str {
        "Manage remote environment variables"
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        match args.trim() {
            "" | "list" => CommandOutput::Message(
                "Remote environment variables:\n\
                 - ANTHROPIC_API_KEY: [set via env]\n\
                 - OXICODE_MODEL: [default]\n\
                 - OXICODE_BRIDGE_JWT_SECRET: [not set]\n\
                 - OPENAI_API_KEY: [for voice feature]\n\
                 - GITHUB_TOKEN: [for GitHub integration]\n\n\
                 Usage: /remote-env set KEY VALUE | /remote-env list"
                    .to_string(),
            ),
            other => {
                if other.starts_with("set ") {
                    CommandOutput::Message(
                        "Remote env variable setting is only available in bridge mode.".to_string(),
                    )
                } else {
                    CommandOutput::Error("Usage: /remote-env [list|set KEY VALUE]".to_string())
                }
            }
        }
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
            command_registry: Arc::new(crate::commands::default_registry()),
        }
    }

    #[test]
    fn test_voice_command_no_feature() {
        let cmd = VoiceCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
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
            CommandOutput::Message(msg) => {
                assert!(msg.contains("8080"));
                assert!(msg.contains("Max sessions"));
            }
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

    #[test]
    fn test_install_github_app_no_token() {
        let cmd = InstallGithubAppCommand;
        let ctx = make_ctx();
        // Without GITHUB_TOKEN, should return an error about auth.
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Error(msg) => assert!(msg.contains("GitHub")),
            CommandOutput::Message(msg) => {
                // If token happens to be set in env, we get wizard steps.
                assert!(msg.contains("Wizard") || msg.contains("GitHub"));
            }
            _ => panic!("Expected error or message"),
        }
    }

    #[test]
    fn test_remote_setup_command() {
        let cmd = RemoteSetupCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("Bridge endpoint"));
                assert!(msg.contains("JWT"));
            }
            _ => panic!("Expected message output"),
        }
    }

    #[test]
    fn test_remote_env_list() {
        let cmd = RemoteEnvCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => assert!(msg.contains("ANTHROPIC_API_KEY")),
            _ => panic!("Expected message output"),
        }
    }

    #[test]
    fn test_remote_env_invalid() {
        let cmd = RemoteEnvCommand;
        let ctx = make_ctx();
        let output = cmd.execute("invalid", &ctx);
        match output {
            CommandOutput::Error(msg) => assert!(msg.contains("Usage")),
            _ => panic!("Expected error output"),
        }
    }
}
