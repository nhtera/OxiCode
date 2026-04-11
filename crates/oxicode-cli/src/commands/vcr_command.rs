//! `/vcr` — record, replay, and list session recordings.
//!
//! Recording state is persisted in `active_skills` as `"vcr:recording"`.
//! Actual capture / replay I/O is stubbed; the list subcommand reads real
//! files from `~/.oxicode/vcr/`.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

const VCR_RECORDING_KEY: &str = "vcr:recording";

/// `/vcr [record|stop|play <name>|list]` — manage session recordings.
pub struct VcrCommand;

impl SlashCommand for VcrCommand {
    fn name(&self) -> &str {
        "vcr"
    }

    fn description(&self) -> &str {
        "Record, replay, and list session recordings"
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let args = args.trim();
        if args.is_empty() {
            return show_usage(ctx);
        }

        // Split into subcommand + optional remainder.
        let (sub, rest) = args
            .split_once(' ')
            .map_or((args, ""), |(s, r)| (s, r.trim()));

        match sub {
            "record" => start_recording(ctx),
            "stop" => stop_recording(ctx),
            "play" => play_recording(rest),
            "list" => list_recordings(),
            other => CommandOutput::Error(format!(
                "Unknown vcr subcommand: '{other}'\n\
                 Usage: /vcr [record|stop|play <name>|list]"
            )),
        }
    }

    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["record", "stop", "play", "list"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Sub-command handlers
// ---------------------------------------------------------------------------

/// Begin a new recording session.
fn start_recording(ctx: &CommandContext) -> CommandOutput {
    let already = ctx
        .state_store
        .current()
        .active_skills
        .iter()
        .any(|s| s == VCR_RECORDING_KEY);

    if already {
        return CommandOutput::Message(
            "VCR is already recording.\n\
             Use /vcr stop to finish the recording."
                .to_string(),
        );
    }

    ctx.state_store.update(|s| {
        s.active_skills.push(VCR_RECORDING_KEY.to_string());
    });

    tracing::debug!("VCR recording started");
    CommandOutput::Message(
        "VCR recording started.\n\
         All tool calls and messages will be captured.\n\
         Use /vcr stop to finish and save the recording."
            .to_string(),
    )
}

/// Stop an in-progress recording and persist it.
fn stop_recording(ctx: &CommandContext) -> CommandOutput {
    let was_recording = ctx
        .state_store
        .current()
        .active_skills
        .iter()
        .any(|s| s == VCR_RECORDING_KEY);

    if !was_recording {
        return CommandOutput::Message(
            "VCR is not recording.\n\
             Use /vcr record to start a new recording."
                .to_string(),
        );
    }

    ctx.state_store.update(|s| {
        s.active_skills.retain(|sk| sk != VCR_RECORDING_KEY);
    });

    // Generate a timestamp-based filename stub.
    let name = chrono_filename();
    tracing::debug!(%name, "VCR recording stopped");
    CommandOutput::Message(format!(
        "VCR recording stopped.\n\
         Saved as: ~/.oxicode/vcr/{name}.vcr.gz (stub — I/O not yet wired)\n\
         Use /vcr list to see all recordings."
    ))
}

/// Play back a named recording (stub).
fn play_recording(name: &str) -> CommandOutput {
    if name.is_empty() {
        return CommandOutput::Error(
            "Usage: /vcr play <name>\n\
             Use /vcr list to see available recordings."
                .to_string(),
        );
    }
    tracing::debug!(%name, "VCR play requested (stub)");
    CommandOutput::Message(format!(
        "Playing recording '{name}' (stub).\n\
         Full replay will be implemented when the VCR engine is wired up."
    ))
}

/// List `.vcr.gz` files in `~/.oxicode/vcr/`.
fn list_recordings() -> CommandOutput {
    let vcr_dir = match dirs::home_dir() {
        Some(h) => h.join(".oxicode").join("vcr"),
        None => return CommandOutput::Error("Could not determine home directory.".to_string()),
    };

    if !vcr_dir.exists() {
        return CommandOutput::Message(
            "No recordings found — ~/.oxicode/vcr/ does not exist yet.\n\
             Use /vcr record to create your first recording."
                .to_string(),
        );
    }

    let entries = match std::fs::read_dir(&vcr_dir) {
        Ok(e) => e,
        Err(err) => return CommandOutput::Error(format!("Failed to read vcr dir: {err}")),
    };

    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name().and_then(|n| n.to_str())?;
            // Accept both .vcr.gz (storage format) and .json (legacy).
            if name.ends_with(".vcr.gz") {
                Some(name.strip_suffix(".vcr.gz").unwrap_or(name).to_string())
            } else if std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            {
                Some(name.strip_suffix(".json").unwrap_or(name).to_string())
            } else {
                None
            }
        })
        .collect();

    if names.is_empty() {
        return CommandOutput::Message(
            "No recordings in ~/.oxicode/vcr/.\n\
             Use /vcr record to create one."
                .to_string(),
        );
    }

    names.sort();
    let mut out = format!("Recordings in ~/.oxicode/vcr/ ({}):\n\n", names.len());
    for name in &names {
        let _ = writeln!(out, "  {name}");
    }
    out.push_str("\nUse /vcr play <name> to replay a recording.");
    CommandOutput::Message(out)
}

/// Show usage help and current recording state.
fn show_usage(ctx: &CommandContext) -> CommandOutput {
    let recording = ctx
        .state_store
        .current()
        .active_skills
        .iter()
        .any(|s| s == VCR_RECORDING_KEY);

    let status = if recording { "RECORDING" } else { "idle" };
    CommandOutput::Message(format!(
        "VCR — session recorder  [status: {status}]\n\n\
         Subcommands:\n\
         /vcr record       — start recording tool calls and messages\n\
         /vcr stop         — stop recording and save\n\
         /vcr play <name>  — replay a saved recording\n\
         /vcr list         — list recordings in ~/.oxicode/vcr/"
    ))
}

/// Generate a simple timestamp-based filename (no external deps).
fn chrono_filename() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("recording-{secs}")
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
    fn test_no_args_shows_usage() {
        let cmd = VcrCommand;
        let ctx = make_ctx();
        match cmd.execute("", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("record")),
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn test_record_stop_cycle() {
        let cmd = VcrCommand;
        let ctx = make_ctx();

        match cmd.execute("record", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("started")),
            _ => panic!("expected Message"),
        }
        // Status in usage should say RECORDING
        match cmd.execute("", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("RECORDING")),
            _ => panic!("expected Message"),
        }
        match cmd.execute("stop", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("stopped")),
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn test_play_no_name_error() {
        let cmd = VcrCommand;
        let ctx = make_ctx();
        match cmd.execute("play", &ctx) {
            CommandOutput::Error(_) => {}
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn test_play_with_name_stub() {
        let cmd = VcrCommand;
        let ctx = make_ctx();
        match cmd.execute("play my-recording", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("my-recording")),
            _ => panic!("expected Message"),
        }
    }
}
