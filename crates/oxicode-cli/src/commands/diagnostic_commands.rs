//! Diagnostic commands: `/heapdump`, `/perf-issue`, `/ant-trace`.
//!
//! Low-overhead debug introspection — no cost when not invoked.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

// ---------------------------------------------------------------------------
// /heapdump — process memory statistics
// ---------------------------------------------------------------------------

/// `/heapdump` — display process memory statistics.
pub struct HeapdumpCommand;

impl SlashCommand for HeapdumpCommand {
    fn name(&self) -> &str {
        "heapdump"
    }
    fn description(&self) -> &str {
        "Show process memory statistics"
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandOutput {
        let mut output = String::with_capacity(512);
        output.push_str("Process Memory Stats\n");
        output.push_str("────────────────────\n");

        #[cfg(target_os = "linux")]
        {
            match read_linux_memory() {
                Ok(info) => {
                    let _ = writeln!(output, "  RSS:        {}", format_bytes(info.rss));
                    let _ = writeln!(output, "  Virtual:    {}", format_bytes(info.vm_size));
                    let _ = writeln!(output, "  Peak RSS:   {}", format_bytes(info.vm_peak));
                    let _ = writeln!(output, "  Data:       {}", format_bytes(info.vm_data));
                }
                Err(e) => {
                    let _ = writeln!(output, "  Error reading /proc/self/status: {e}");
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            match read_macos_memory() {
                Ok((rss, virtual_size)) => {
                    let _ = writeln!(output, "  RSS:        {}", format_bytes(rss));
                    let _ = writeln!(output, "  Virtual:    {}", format_bytes(virtual_size));
                }
                Err(e) => {
                    let _ = writeln!(output, "  Error: {e}");
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            output.push_str("  Memory stats not available on Windows (use Task Manager).\n");
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            output.push_str("  Memory stats not available on this platform.\n");
        }

        // Cross-platform: PID and uptime estimate.
        let _ = writeln!(output, "\n  PID:        {}", std::process::id());

        CommandOutput::Message(output)
    }
}

/// Format bytes into human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        #[allow(clippy::cast_precision_loss)]
        let gb = bytes as f64 / GB as f64;
        format!("{gb:.1} GB")
    } else if bytes >= MB {
        #[allow(clippy::cast_precision_loss)]
        let mb = bytes as f64 / MB as f64;
        format!("{mb:.1} MB")
    } else if bytes >= KB {
        #[allow(clippy::cast_precision_loss)]
        let kb = bytes as f64 / KB as f64;
        format!("{kb:.1} KB")
    } else {
        format!("{bytes} B")
    }
}

#[cfg(target_os = "linux")]
struct LinuxMemInfo {
    rss: u64,
    vm_size: u64,
    vm_peak: u64,
    vm_data: u64,
}

#[cfg(target_os = "linux")]
fn read_linux_memory() -> Result<LinuxMemInfo, String> {
    let content = std::fs::read_to_string("/proc/self/status").map_err(|e| e.to_string())?;

    let mut info = LinuxMemInfo {
        rss: 0,
        vm_size: 0,
        vm_peak: 0,
        vm_data: 0,
    };

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let value_kb: u64 = parts[1].parse().unwrap_or(0);
        let value_bytes = value_kb * 1024;

        match parts[0] {
            "VmRSS:" => info.rss = value_bytes,
            "VmSize:" => info.vm_size = value_bytes,
            "VmPeak:" => info.vm_peak = value_bytes,
            "VmData:" => info.vm_data = value_bytes,
            _ => {}
        }
    }

    Ok(info)
}

#[cfg(target_os = "macos")]
fn read_macos_memory() -> Result<(u64, u64), String> {
    // Use `ps` as a portable fallback (avoids libc FFI complexity).
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=,vsz=", "-p", &std::process::id().to_string()])
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err("ps command failed".to_string());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.len() < 2 {
        return Err("unexpected ps output".to_string());
    }

    let rss_kb: u64 = parts[0].parse().unwrap_or(0);
    let vsz_kb: u64 = parts[1].parse().unwrap_or(0);

    Ok((rss_kb * 1024, vsz_kb * 1024))
}

// ---------------------------------------------------------------------------
// /perf-issue — performance timing info
// ---------------------------------------------------------------------------

/// `/perf-issue` — display performance and timing information.
pub struct PerfIssueCommand;

impl SlashCommand for PerfIssueCommand {
    fn name(&self) -> &str {
        "perf-issue"
    }
    fn description(&self) -> &str {
        "Show performance timing information"
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let msg_count = state.messages.len();
        let usage = &state.total_usage;

        // Calculate basic performance metrics from available data.
        let total_tokens = usage.input_tokens + usage.output_tokens;
        let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
        let cache_created = usage.cache_creation_input_tokens.unwrap_or(0);

        #[allow(clippy::cast_precision_loss)]
        let cache_hit_rate = if usage.input_tokens > 0 {
            (f64::from(cache_read) / f64::from(usage.input_tokens) * 100.0).min(100.0)
        } else {
            0.0
        };

        // Estimate avg tokens per message.
        #[allow(clippy::cast_precision_loss)]
        let avg_tokens_per_msg = if msg_count > 0 {
            f64::from(total_tokens) / msg_count as f64
        } else {
            0.0
        };

        let mut output = String::with_capacity(512);
        output.push_str("Performance Report\n");
        output.push_str("──────────────────\n");
        let _ = writeln!(output, "  Model:              {}", ctx.model);
        let _ = writeln!(output, "  Messages:           {msg_count}");
        let _ = writeln!(output, "  Total tokens:       {total_tokens}");
        let _ = writeln!(output, "  Avg tokens/msg:     {avg_tokens_per_msg:.0}");
        let _ = writeln!(output, "  Input tokens:       {}", usage.input_tokens);
        let _ = writeln!(output, "  Output tokens:      {}", usage.output_tokens);
        let _ = writeln!(output, "  Cache read:         {cache_read}");
        let _ = writeln!(output, "  Cache created:      {cache_created}");
        let _ = writeln!(output, "  Cache hit rate:     {cache_hit_rate:.1}%");
        let _ = writeln!(output);
        let _ = writeln!(
            output,
            "  Active agents:      {}",
            state.active_agents.len()
        );
        let _ = writeln!(
            output,
            "  Background tasks:   {}",
            state.background_tasks.len()
        );
        let _ = writeln!(
            output,
            "  Active skills:      {}",
            state.active_skills.len()
        );
        output.push_str("\n  Note: latency metrics require instrumented API calls.\n  Use /debug for per-call tracing.");

        CommandOutput::Message(output)
    }
}

// ---------------------------------------------------------------------------
// /ant-trace — execution trace
// ---------------------------------------------------------------------------

/// `/ant-trace` — display execution trace of recent messages.
pub struct AntTraceCommand;

impl SlashCommand for AntTraceCommand {
    fn name(&self) -> &str {
        "ant-trace"
    }
    fn description(&self) -> &str {
        "Show execution trace of recent activity"
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let state = ctx.state_store.current();
        let messages = &state.messages;

        // Parse optional count argument (default 20).
        let count: usize = args.trim().parse().unwrap_or(20);
        let count = count.min(100); // Cap at 100

        if messages.is_empty() {
            return CommandOutput::Message(
                "No messages in current session. Start a conversation to see traces.".to_string(),
            );
        }

        let recent: Vec<&oxicode_common::Message> = messages
            .iter()
            .rev()
            .take(count)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let mut output = String::with_capacity(2048);
        output.push_str("Execution Trace\n");
        output.push_str("───────────────────────────────────────────────────────\n");
        let _ = writeln!(output, "  {:>4}  {:<10}  Preview", "#", "Role");
        output.push_str("  ────  ──────────  ──────────────────────────────────\n");

        let offset = messages.len().saturating_sub(recent.len());
        for (i, msg) in recent.iter().enumerate() {
            let role = format!("{:?}", msg.role).to_lowercase();
            let text = msg.text();
            let preview = if text.chars().count() > 50 {
                let truncated: String = text.chars().take(50).collect();
                format!("{truncated}...")
            } else {
                text.clone()
            };
            // Clean up newlines for display.
            let preview = preview.replace('\n', " ");
            let _ = writeln!(output, "  {:>4}  {:<10}  {preview}", offset + i + 1, role);
        }

        let _ = writeln!(
            output,
            "\n  Showing {}/{} messages",
            recent.len(),
            messages.len()
        );

        // Show agent/task summary if any.
        if !state.active_agents.is_empty() {
            output.push_str("\n  Active agents:\n");
            for agent in &state.active_agents {
                let _ = writeln!(output, "    - {} ({})", agent.name, agent.status);
            }
        }

        if !state.background_tasks.is_empty() {
            output.push_str("\n  Background tasks:\n");
            for task in &state.background_tasks {
                let _ = writeln!(
                    output,
                    "    - [{}] {} ({})",
                    task.task_type, task.command_preview, task.status
                );
            }
        }

        CommandOutput::Message(output)
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
            model: "test-model".to_string(),
            provider_name: "test".to_string(),
            session_id: "test".to_string(),
        }
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
        assert_eq!(format_bytes(5_242_880), "5.0 MB");
    }

    #[test]
    fn test_heapdump_command_metadata() {
        let cmd = HeapdumpCommand;
        assert_eq!(cmd.name(), "heapdump");
        assert!(!cmd.description().is_empty());
    }

    #[test]
    fn test_heapdump_command_executes() {
        let cmd = HeapdumpCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("Process Memory Stats"));
                assert!(msg.contains("PID"));
            }
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_perf_issue_command_metadata() {
        let cmd = PerfIssueCommand;
        assert_eq!(cmd.name(), "perf-issue");
        assert!(!cmd.description().is_empty());
    }

    #[test]
    fn test_perf_issue_command_executes() {
        let cmd = PerfIssueCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("Performance Report"));
                assert!(msg.contains("test-model"));
                assert!(msg.contains("Total tokens"));
            }
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_ant_trace_no_messages() {
        let cmd = AntTraceCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("No messages"));
            }
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_ant_trace_with_messages() {
        let ctx = make_ctx();
        ctx.state_store
            .push_message(oxicode_common::Message::user("hello world"));
        ctx.state_store
            .push_message(oxicode_common::Message::user("hi there"));

        let cmd = AntTraceCommand;
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("Execution Trace"));
                assert!(msg.contains("hello world"));
                assert!(msg.contains("2/2"));
            }
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_ant_trace_count_arg() {
        let ctx = make_ctx();
        for i in 0..10 {
            ctx.state_store
                .push_message(oxicode_common::Message::user(&format!("msg {i}")));
        }

        let cmd = AntTraceCommand;
        let output = cmd.execute("3", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("3/10"));
            }
            _ => panic!("Expected message"),
        }
    }
}
