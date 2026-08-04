pub mod auth;
#[allow(dead_code)] // Bridge API — consumers will use these as bridge matures.
mod bridge;
mod commands;
mod completions;
pub mod github;
pub mod github_service;
pub mod oauth;
mod onboarding;
pub mod remote;
mod server;
mod server_handler;
mod server_protocol;
mod structured_output;
pub mod telemetry;
pub mod telemetry_pipeline;
pub mod voice;

use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use oxicode_api::ProviderRouter;
use oxicode_common::{ContentBlock, Message, Role};
use oxicode_config::Settings;
use oxicode_core::{Conversation, QueryEngine};
use oxicode_hooks::HookManager;
use oxicode_permissions::pipeline::{PermissionMode, PermissionPipeline};
use oxicode_session::Session;
use oxicode_state::{AppState, StateStore};
use oxicode_tools::ToolContext;
use oxicode_tui::{App, CoreEvent, UiEvent};
use tokio::sync::mpsc;

use structured_output::NdjsonWriter;

/// Output format for the CLI.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum OutputFormat {
    /// Human-readable text (default, interactive TUI).
    #[default]
    Text,
    /// NDJSON structured output (one JSON object per line to stdout).
    Json,
}

/// Permission mode override from the command line.
///
/// Mirrors Claude Code's `--permission-mode` flag values so users can pass
/// the same flags they use with `claude`.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum PermissionModeArg {
    /// Default — read-only auto-allowed, others prompt.
    Default,
    /// Auto-accept file edits but still prompt for shell.
    AcceptEdits,
    /// Bypass all permission checks (alias for `--dangerously-skip-permissions`).
    BypassPermissions,
    /// Plan mode — read-only investigation, no edits/exec until exit.
    Plan,
}

impl PermissionModeArg {
    /// Translate to the settings string form parsed by `PermissionMode::parse`.
    fn as_settings_str(self) -> &'static str {
        match self {
            Self::Default | Self::Plan => "default",
            Self::AcceptEdits => "approval_only",
            Self::BypassPermissions => "bypass",
        }
    }
}

/// `OxiCode` — A Rust-powered CLI agent for software engineering.
#[derive(Parser, Debug)]
#[command(name = "oxicode", version, about)]
#[allow(clippy::struct_excessive_bools)] // CLI flags are inherently boolean
struct Cli {
    /// Model to use (e.g., claude-sonnet-4-20250514).
    #[arg(short, long)]
    model: Option<String>,

    // C3 FIX: Removed --api-key flag. Use ANTHROPIC_API_KEY env var or config file.
    /// Config directory path.
    #[arg(long)]
    config_dir: Option<String>,

    /// Resume a previous session by full ID (exact match only).
    #[arg(short, long)]
    session: Option<String>,

    /// Resume a specific branch within a session tree (UUID format).
    ///
    /// Use with `--session <session_id>` to resume a particular branch.
    /// When omitted, the session's last-active branch is loaded.
    #[arg(long, value_name = "BRANCH_ID")]
    branch: Option<String>,

    /// Resume a previous conversation (Claude Code parity).
    ///
    /// With no value: opens an interactive session picker on startup.
    /// With a value: accepts a UUID prefix or preview substring — single
    /// match resumes that session, ambiguous matches exit with an error.
    #[arg(
        short = 'r',
        long,
        num_args = 0..=1,
        default_missing_value = "",
        value_name = "QUERY"
    )]
    resume: Option<String>,

    /// Send a single message and exit (non-interactive mode).
    #[arg(short = 'p', long)]
    prompt: Option<String>,

    /// Output format: text (default) or json (NDJSON structured output).
    #[arg(short, long, default_value = "text")]
    output: OutputFormat,

    /// Generate shell completions for the given shell and exit.
    #[arg(long, value_name = "SHELL")]
    completions: Option<clap_complete::Shell>,

    /// Generate man page to stdout and exit.
    #[arg(long)]
    man_page: bool,

    /// Run in agent mode (subagent receives config via stdin).
    #[arg(long, value_name = "AGENT_ID", hide = true)]
    agent_mode: Option<String>,

    /// Run as a long-running JSON-RPC server for IDE integration.
    #[arg(long)]
    server: bool,

    /// Run as a daemon: TCP listener for IDE connections (Phase B bridge).
    #[arg(long)]
    daemon: bool,

    /// Run as a headless bridge server for cloud deployment.
    #[arg(long, hide = true)]
    bridge: bool,

    /// Port for bridge mode (default: 8080).
    #[arg(long, default_value = "8080", hide = true)]
    port: u16,

    /// Skip first-run onboarding wizard.
    #[arg(long)]
    no_onboard: bool,

    /// Run as an MCP server (expose tools via stdio JSON-RPC).
    #[arg(long)]
    mcp: bool,

    /// Permission mode override (overrides settings.permission_mode for this run).
    #[arg(long, value_enum)]
    permission_mode: Option<PermissionModeArg>,

    /// Skip ALL permission prompts (alias for --permission-mode=bypass-permissions).
    /// Use with caution — runs every tool without confirmation.
    #[arg(long, conflicts_with = "permission_mode")]
    dangerously_skip_permissions: bool,

    /// Comma-separated list of tool names that should always be allowed
    /// (e.g. `--allowed-tools=bash,file_write`). Glob `*` is supported.
    #[arg(long, value_delimiter = ',')]
    allowed_tools: Vec<String>,

    /// Comma-separated list of tool names that should always be denied.
    #[arg(long, value_delimiter = ',')]
    disallowed_tools: Vec<String>,
}

/// Reject session IDs that contain path-separator characters or `..`.
///
/// This mirrors the guard in `oxicode_session::load_session` and must be
/// applied before any `Path::join(session_id)` call so that a malicious
/// value like `"../../etc/passwd"` cannot escape the sessions directory.
fn validate_session_id(id: &str) -> Result<(), String> {
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(format!(
            "Invalid session ID '{id}': must not contain path separators or '..'"
        ));
    }
    Ok(())
}

/// Resolve the effective permission mode from CLI flags + settings.
/// CLI flags override settings.toml. `--dangerously-skip-permissions` is
/// equivalent to `--permission-mode=bypass-permissions`.
fn resolve_permission_mode(cli: &Cli, settings_value: &str) -> PermissionMode {
    if cli.dangerously_skip_permissions {
        return PermissionMode::Bypass;
    }
    if let Some(mode) = cli.permission_mode {
        return PermissionMode::parse(mode.as_settings_str());
    }
    PermissionMode::parse(settings_value)
}

/// Build `PermissionRule`s from `--allowed-tools` and `--disallowed-tools`.
/// Deny rules are pushed first so they take precedence in the matcher.
fn build_cli_permission_rules(
    allowed: &[String],
    disallowed: &[String],
) -> Vec<oxicode_permissions::rules::PermissionRule> {
    use oxicode_permissions::rules::PermissionRule;
    let mut rules = Vec::with_capacity(allowed.len() + disallowed.len());
    for tool in disallowed {
        rules.push(PermissionRule::deny(tool, None));
    }
    for tool in allowed {
        rules.push(PermissionRule::allow(tool, None));
    }
    rules
}

/// Resolve project memory for injection into the system prompt.
///
/// Resolution order:
/// 1. Discover git root via `git2::Repository::discover`; fall back to `cwd`.
/// 2. Compute the project memory dir (`~/.oxicode/projects/{key}/memory/`).
/// 3. Return `None` when the dir is missing/empty (preserves prior behavior).
/// 4. Read `MEMORY.md` entrypoint (truncated to 200 lines / 25 KB by `memdir`).
/// 5. Scan additional `.md` files (capped at 200, 50 KB each).
/// 6. Build an index of additional files and combine with entrypoint.
/// 7. Cap the combined result at 100 KB before returning.
///
/// Errors are logged and skipped — this function never panics on malformed
/// frontmatter or filesystem failures.
fn build_project_memory(cwd: &std::path::Path) -> Option<String> {
    use oxicode_session::{memdir, memory_scanner};

    // Resolve git root; fall back to cwd on shallow clones, detached worktrees, etc.
    let root = git2::Repository::discover(cwd)
        .ok()
        .and_then(|repo| {
            repo.workdir()
                .map(std::path::Path::to_path_buf)
                .or_else(|| repo.path().parent().map(std::path::Path::to_path_buf))
        })
        .unwrap_or_else(|| cwd.to_path_buf());

    let memory_dir = memdir::project_memory_dir(&root);

    // Bail early when the memory dir doesn't exist.
    if !memory_dir.exists() {
        return None;
    }

    // Read the MEMORY.md entrypoint (already capped at 200 lines / 25 KB).
    let entrypoint = memdir::read_entrypoint(&memory_dir)
        .map(|(content, _truncated)| content)
        .unwrap_or_default();

    // Scan additional memory files; tolerate empty dir or read errors.
    let headers = match memory_scanner::scan_memory_files(&memory_dir) {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("memory scan failed for {}: {e}", memory_dir.display());
            Vec::new()
        }
    };

    // Nothing to inject.
    if entrypoint.is_empty() && headers.is_empty() {
        return None;
    }

    // Build combined memory string.
    let mut combined = String::with_capacity(entrypoint.len() + 512);

    if !entrypoint.is_empty() {
        combined.push_str(&entrypoint);
    }

    if !headers.is_empty() {
        let index = memory_scanner::build_memory_index(&headers);
        if !index.is_empty() {
            if combined.is_empty() {
                combined.push_str("## Additional Memory Files\n\n");
            } else {
                combined.push_str("\n\n## Additional Memory Files\n\n");
            }
            combined.push_str(&index);
        }
    }

    if combined.is_empty() {
        return None;
    }

    // Cap total injection at 100 KB.
    let max_injection_bytes: usize = 100 * 1024;
    if combined.len() > max_injection_bytes {
        let mut end = max_injection_bytes;
        while end > 0 && !combined.is_char_boundary(end) {
            end -= 1;
        }
        combined.truncate(end);
        combined.push_str("\n\n<!-- Project memory truncated at 100 KB -->");
    }

    Some(combined)
}

#[tokio::main]
#[allow(clippy::too_many_lines)] // CLI entry point with mode dispatch
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Fast-exit: agent mode (subagent spawned by parent)
    if let Some(ref agent_id) = cli.agent_mode {
        return run_agent_mode(agent_id).await;
    }

    // Fast-exit: generate shell completions
    if let Some(shell) = cli.completions {
        completions::generate_completions(shell, &mut std::io::stdout());
        return Ok(());
    }

    // Fast-exit: generate man page
    if cli.man_page {
        completions::generate_man_page(&mut std::io::stdout())?;
        return Ok(());
    }

    // Fast-exit: MCP server mode (expose tools via stdio JSON-RPC)
    if cli.mcp {
        return run_mcp_server_mode().await;
    }

    // First-run onboarding wizard (skip with --no-onboard or non-interactive modes).
    if !cli.no_onboard
        && cli.prompt.is_none()
        && !cli.server
        && !cli.daemon
        && onboarding::should_onboard()
    {
        onboarding::run_onboarding();
    }

    // Setup tracing — write to log file, NOT stderr.
    // stderr leaks into crossterm's alternate screen and corrupts TUI output.
    // With `telemetry-otlp` feature: OTLP layer is composed in addition to the
    // file writer. Without it: behaviour is identical to before (file only).
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("oxicode");
    std::fs::create_dir_all(&log_dir).ok();
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("oxicode.log"))
        .unwrap_or_else(|_| {
            std::fs::File::options()
                .write(true)
                .open("/dev/null")
                .expect("failed to open /dev/null")
        });

    // Build the base Registry + fmt layer, then optionally compose the OTLP layer.
    {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        let env_filter = tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("oxicode=info".parse()?);
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::sync::Mutex::new(log_file))
            .with_ansi(false);
        let base = tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer);

        // Conditionally add the OTLP layer (no-op pass-through when feature is off).
        telemetry::install_otlp_layer(base).init();
    }

    // Load config (env vars and TOML, no CLI secret)
    let mut settings = oxicode_config::load_settings(cli.config_dir.as_deref());

    if let Some(ref model) = cli.model {
        settings.model = model.clone();
    }

    // Resolve auth source: OAuth token > env var > config > none.
    let auth_source = oauth::resolve_auth_source().await;
    let auth_label = auth_source.display_label();
    let oauth_token = match &auth_source {
        oauth::AuthSource::OAuth { token, .. } => Some(token.clone()),
        _ => None,
    };

    // Build provider router (with OAuth token if available).
    let router = ProviderRouter::from_env_with_oauth(oauth_token);
    let resolved = match router.resolve(&settings.model) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };
    let provider = resolved.provider;

    // Load CLAUDE.md / OXICODE.md
    let cwd = std::env::current_dir()?;
    let (global_md, project_md) = oxicode_config::load_claude_md(&cwd);

    // Discover and initialize skills.
    let user_skills_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".oxicode")
        .join("skills");
    let project_skills_dir = cwd.join(".oxicode").join("skills");
    let skill_discovery = oxicode_skills::SkillDiscovery::new(user_skills_dir, project_skills_dir);
    let discovered_skills = skill_discovery.discover();
    let skill_executor = Arc::new(oxicode_skills::SkillExecutor::new(discovered_skills));

    // Build skills prompt for system prompt injection.
    let skill_activation_ctx = oxicode_skills::ActivationContext {
        current_file: None,
        user_input: None,
    };
    let skills_prompt = skill_executor.build_skills_prompt(&skill_activation_ctx);

    let system_prompt = oxicode_core::system_prompt::assemble_system_prompt(
        global_md.as_deref(),
        project_md.as_deref(),
        skills_prompt.as_deref(),
        build_project_memory(&cwd).as_deref(),
    );

    // Resolve `-r/--resume`. With a query, fuzzy-match before boot. Without
    // one, signal the TUI to open the session picker immediately.
    let mut open_picker_on_start = false;
    let resume_id: Option<String> = match cli.resume.as_deref() {
        Some("") => {
            open_picker_on_start = true;
            None
        }
        Some(query) => match resolve_resume_arg(query, "") {
            ResumeLookup::Found(s) => Some(s.id),
            ResumeLookup::NotFound => {
                eprintln!("Session '{query}' not found.");
                std::process::exit(1);
            }
            ResumeLookup::Multiple(n) => {
                eprintln!(
                    "Found {n} sessions matching '{query}'. \
                         Use `oxicode --resume` to pick one interactively."
                );
                std::process::exit(1);
            }
            ResumeLookup::SameAsCurrent => None,
        },
        None => None,
    };

    let load_id = resume_id.as_deref().or(cli.session.as_deref());
    let mut session = if let Some(session_id) = load_id {
        // When --branch is also specified, load that specific branch from the tree.
        if let Some(ref branch_id) = cli.branch {
            // Validate UUID format.
            if branch_id.parse::<uuid::Uuid>().is_err() {
                eprintln!(
                    "Invalid --branch value '{branch_id}': must be a valid UUID \
                     (e.g. a1b2c3d4-e5f6-7890-abcd-ef1234567890)."
                );
                std::process::exit(1);
            }
            // Reject path traversal in the user-supplied session ID (same guard as
            // load_session in oxicode-session/src/lib.rs:172).
            if let Err(e) = validate_session_id(session_id) {
                eprintln!("{e}");
                std::process::exit(1);
            }
            // Load the branch file directly: sessions/{session_id}/{branch_id}.json
            let branch_path = oxicode_session::sessions_dir(None)
                .join(session_id)
                .join(format!("{branch_id}.json"));
            if let Some(s) = std::fs::read_to_string(&branch_path)
                .ok()
                .and_then(|s| serde_json::from_str::<Session>(&s).ok())
            {
                tracing::info!("Resumed session {} branch {}", session_id, branch_id);
                s
            } else {
                eprintln!("Branch '{branch_id}' not found in session '{session_id}'.");
                std::process::exit(1);
            }
        } else {
            match oxicode_session::load_session(session_id, None) {
                Ok(s) => {
                    tracing::info!("Resumed session {}", session_id);
                    s
                }
                Err(e) => {
                    eprintln!("Failed to load session: {e}");
                    std::process::exit(1);
                }
            }
        }
    } else {
        Session::new(&settings.model)
    };

    // Initialize state with the (possibly resumed) session id so session-scoped
    // resources (image cache dir, cost tracker) stay aligned after resume.
    let mut initial_state = AppState {
        current_model: settings.model.clone(),
        auth_label,
        ..AppState::default()
    };
    initial_state.session_id = session.id.clone();
    initial_state.cost_tracker = oxicode_state::cost_tracker::CostTracker::new(session.id.clone());
    let state_store = Arc::new(StateStore::new(initial_state));

    let tool_registry = Arc::new(oxicode_tools::default_registry());
    let permission_mode = resolve_permission_mode(&cli, &settings.permission_mode);
    let cli_rules = build_cli_permission_rules(&cli.allowed_tools, &cli.disallowed_tools);
    let permission_pipeline = Arc::new(PermissionPipeline::new(permission_mode, cli_rules));

    // Initialize HookManager from layered settings (Claude Code parity:
    // ~/.claude/settings.json + project .claude/settings.json + .oxicode overlay).
    let mut hook_mgr = HookManager::from_settings();
    hook_mgr.set_model(&settings.model);
    hook_mgr.set_session_id(&session.id);
    hook_mgr.set_cwd(cwd.to_string_lossy());
    hook_mgr.set_permission_mode(match permission_mode {
        PermissionMode::Bypass => "bypass",
        PermissionMode::ApprovalOnly => "approval_only",
        PermissionMode::Default => "default",
    });
    let transcript_path = oxicode_session::sessions_dir(None)
        .join(format!("{}.json", session.id))
        .to_string_lossy()
        .into_owned();
    hook_mgr.set_transcript_path(transcript_path);
    let hook_manager = Arc::new(hook_mgr);

    // Initialize MCP servers from config.
    let mut mcp_manager = oxicode_mcp::McpServerManager::new();
    let mcp_config = oxicode_mcp::McpConfig::load();
    let started = mcp_manager.start_from_config(&mcp_config).await;
    if !started.is_empty() {
        tracing::info!("MCP servers started: {}", started.join(", "));
    }

    // Route MCP `elicitation/create` requests to the TUI dialog.
    // The App receiver is installed on `app` after construction below.
    let (elicitation_tx, elicitation_rx) = tokio::sync::mpsc::unbounded_channel();
    mcp_manager.set_elicitation_handler(Arc::new(
        oxicode_mcp::ChannelElicitationHandler::from_sender(elicitation_tx),
    ));

    let mcp_ref = std::sync::Arc::new(mcp_manager);
    let file_state_ref =
        std::sync::Arc::new(oxicode_tools::file_state_tracker::FileStateTracker::default());
    // Populate extra_working_dirs from resumed session (if any) so the tool
    // context is immediately aware of previously added directories.
    let session_extra_dirs = session.working_dirs.clone();

    // Sync session's working_dirs into AppState so commands see the restored set.
    if !session_extra_dirs.is_empty() {
        state_store.update(|s| {
            for dir in &session_extra_dirs {
                if !s.working_dirs.contains(dir) {
                    s.working_dirs.push(dir.clone());
                }
            }
        });
    }

    let tool_context = ToolContext {
        working_dir: cwd.clone(),
        extra_working_dirs: session_extra_dirs,
        file_state: file_state_ref.clone(),
        task_manager: std::sync::Arc::new(std::sync::Mutex::new(
            oxicode_tasks::TaskManager::default(),
        )),
        task_abort_handles: std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::HashMap::new(),
        )),
        mcp_manager: mcp_ref.clone(),
        skill_executor: Some(skill_executor),
        team_manager: std::sync::Arc::new(
            std::sync::Mutex::new(oxicode_agents::TeamManager::new()),
        ),
        bash_processes: std::sync::Arc::new(
            std::sync::Mutex::new(std::collections::HashMap::new()),
        ),
        hook_manager: Some(hook_manager.clone()),
        permission_mode,
    };

    let engine = Arc::new(QueryEngine::new(
        provider,
        state_store.clone(),
        tool_registry,
        permission_pipeline,
        tool_context,
        resolved.model,
        settings.max_tokens,
        system_prompt,
    ));

    // Fire SessionStart hook.
    hook_manager
        .fire_simple(oxicode_hooks::HookEvent::SessionStart)
        .await;

    if let Some(prompt) = cli.prompt {
        let result = match cli.output {
            OutputFormat::Json => {
                run_single_prompt_json(engine, &mut session, &prompt, &settings.model).await
            }
            OutputFormat::Text => run_single_prompt(engine, &mut session, &prompt).await,
        };
        hook_manager
            .fire_simple(oxicode_hooks::HookEvent::SessionEnd)
            .await;
        mcp_ref.shutdown_all().await;
        return result;
    }

    // Server mode: long-running JSON-RPC service for IDE extensions.
    if cli.server {
        let result = server::run_server(engine, settings.model).await;
        mcp_ref.shutdown_all().await;
        return result;
    }

    // Daemon mode: TCP listener for IDE connections (Phase B bridge).
    if cli.daemon {
        use bridge::daemon_listener;

        // Check for existing daemon.
        if let Some(existing) = daemon_listener::is_daemon_running() {
            eprintln!(
                "Error: Daemon already running (PID {}, port {})",
                existing.pid, existing.port
            );
            std::process::exit(1);
        }

        let daemon_config = daemon_listener::DaemonConfig {
            port: cli.port,
            ..daemon_listener::DaemonConfig::default()
        };

        eprintln!(
            "Daemon mode starting on {} (max {} connections)",
            daemon_config.socket_addr(),
            daemon_config.max_connections,
        );

        // Write lockfile (port will be updated after bind if port=0).
        let actual_port = if daemon_config.port == 0 {
            0
        } else {
            daemon_config.port
        };
        if actual_port > 0 {
            if let Err(e) =
                daemon_listener::write_lockfile(actual_port, &daemon_config.bind_address)
            {
                eprintln!("Warning: Failed to write lockfile: {e}");
            }
        }

        eprintln!("Press Ctrl+C to stop.");
        tokio::signal::ctrl_c().await.ok();

        daemon_listener::remove_lockfile();
        mcp_ref.shutdown_all().await;
        return Ok(());
    }

    // Bridge mode: headless WebSocket server for IDE extensions and cloud deployment.
    if cli.bridge {
        #[cfg(feature = "bridge")]
        {
            use std::sync::Arc;
            use tokio::sync::Mutex;

            // Read env-configurable limits (with per-spec defaults).
            let max_sessions = std::env::var("OXICODE_BRIDGE_MAX_SESSIONS")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(16);
            let idle_timeout_secs = std::env::var("OXICODE_BRIDGE_IDLE_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(600);

            let bridge_config = remote::BridgeConfig {
                port: cli.port,
                max_sessions,
                idle_timeout_secs,
                ..remote::BridgeConfig::default()
            };

            eprintln!(
                "Bridge mode starting on {} (max {} sessions, idle timeout {}s)",
                bridge_config.socket_addr(),
                bridge_config.max_sessions,
                bridge_config.idle_timeout_secs,
            );

            let pool = Arc::new(Mutex::new(remote::session_pool::SessionPool::new(
                bridge_config.max_sessions,
                bridge_config.idle_timeout_secs,
            )));

            tracing::info!(
                port = cli.port,
                bind = %bridge_config.bind_address,
                max_sessions,
                idle_timeout_secs,
                "Bridge server starting (WebSocket + JWT)"
            );

            let result = remote::bridge::run_bridge(bridge_config, pool, engine).await;
            mcp_ref.shutdown_all().await;
            return result;
        }

        #[cfg(not(feature = "bridge"))]
        {
            eprintln!(
                "Bridge mode requires the `bridge` feature flag. \
                 Rebuild with: cargo build --features bridge"
            );
            std::process::exit(1);
        }
    }

    if matches!(cli.output, OutputFormat::Json) {
        eprintln!("Warning: --output json is only supported with --prompt (non-interactive mode).");
    }

    let result = run_tui(
        engine,
        state_store,
        &mut session,
        &settings,
        hook_manager.clone(),
        file_state_ref,
        open_picker_on_start,
        elicitation_rx,
    )
    .await;
    hook_manager
        .fire_simple(oxicode_hooks::HookEvent::SessionEnd)
        .await;
    mcp_ref.shutdown_all().await;

    // Show resume session hint after TUI exits.
    if !session.messages.is_empty() {
        eprintln!("\nResume this session with:");
        eprintln!("  oxicode --session {}", session.id);
    }

    result
}

/// Run a single prompt and exit.
async fn run_single_prompt(
    engine: Arc<QueryEngine>,
    session: &mut Session,
    prompt: &str,
) -> Result<()> {
    let mut conversation = Conversation::new();
    for msg in &session.messages {
        conversation.push(msg.clone());
    }

    let user_msg = Message::user(prompt);
    session.push_message(user_msg.clone());
    conversation.push(user_msg);

    match engine.execute_turn(&mut conversation, None).await {
        Ok(assistant_msg) => {
            println!("{}", assistant_msg.text());
            session.push_message(assistant_msg);
            oxicode_session::save_session(session, None)?;
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Run a single prompt with NDJSON structured output.
///
/// Streams `TurnEvent`s from the engine to stdout in real-time so consumers
/// see tool_use/tool_result/hook events as they happen, not just the final
/// assistant message. In `-p` mode without a permission bypass flag, any
/// `PermissionAsk` is auto-denied (no human in the loop).
async fn run_single_prompt_json(
    engine: Arc<QueryEngine>,
    session: &mut Session,
    prompt: &str,
    model: &str,
) -> Result<()> {
    let mut writer = NdjsonWriter::new();

    writer.session_start(&session.id, model)?;
    writer.user_message(prompt)?;

    let mut conversation = Conversation::new();
    for msg in &session.messages {
        conversation.push(msg.clone());
    }

    let user_msg = Message::user(prompt);
    session.push_message(user_msg.clone());
    conversation.push(user_msg);

    let (event_tx, mut event_rx) = mpsc::channel::<oxicode_core::TurnEvent>(64);

    let drain_handle = tokio::spawn(async move {
        let mut writer = NdjsonWriter::new();
        let mut text_buf = String::new();
        while let Some(event) = event_rx.recv().await {
            if let Err(e) = stream_event_to_ndjson(&mut writer, &mut text_buf, event) {
                eprintln!("ndjson stream error: {e}");
                break;
            }
        }
        // Flush any remaining buffered text as a final assistant_text.
        if !text_buf.is_empty() {
            let _ = writer.assistant_text(&text_buf);
        }
    });

    let result = engine
        .execute_turn(&mut conversation, Some(&event_tx))
        .await;
    drop(event_tx);
    let _ = drain_handle.await;

    let mut writer = NdjsonWriter::new();
    match result {
        Ok(assistant_msg) => {
            if let Some(usage) = &assistant_msg.usage {
                writer.emit(&structured_output::NdjsonEvent::Usage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                })?;
            }

            let stop_reason = assistant_msg
                .stop_reason
                .map_or("end_turn".to_string(), |r| format!("{r:?}").to_lowercase());
            writer.turn_complete(&stop_reason)?;

            session.push_message(assistant_msg);
            oxicode_session::save_session(session, None)?;
        }
        Err(e) => {
            writer.error(&e.to_string())?;
            writer.session_end("error")?;
            std::process::exit(1);
        }
    }

    writer.session_end("complete")?;
    Ok(())
}

/// Convert one `TurnEvent` to an NDJSON line. Buffers text deltas so the
/// final assistant text can be emitted as a single `assistant_text` event
/// at end-of-stream while individual `text_delta` events stream live.
fn stream_event_to_ndjson(
    writer: &mut NdjsonWriter,
    text_buf: &mut String,
    event: oxicode_core::TurnEvent,
) -> std::io::Result<()> {
    use oxicode_core::TurnEvent;
    match event {
        TurnEvent::TextDelta(text) => {
            text_buf.push_str(&text);
            writer.emit(&structured_output::NdjsonEvent::TextDelta { text })
        }
        TurnEvent::ThinkingDelta(text) => {
            writer.emit(&structured_output::NdjsonEvent::ThinkingDelta { text })
        }
        TurnEvent::TurnStart | TurnEvent::TurnEnd => Ok(()),
        TurnEvent::ToolUseStart { id, name, input } => writer.tool_use(&id, &name, &input),
        TurnEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => writer.tool_result(&tool_use_id, &content, is_error),
        TurnEvent::PermissionAsk {
            tool_name,
            input_summary,
            prompt,
            reply_tx,
        } => {
            // Non-interactive `-p` mode: auto-deny so engine doesn't hang.
            let _ = reply_tx.send(oxicode_common::PermissionResponse::Deny);
            writer.emit(&structured_output::NdjsonEvent::PermissionAsk {
                tool_name,
                input_summary,
                prompt,
                decision: "denied".to_string(),
            })
        }
        TurnEvent::Error(message) => {
            writer.emit(&structured_output::NdjsonEvent::Error { message })
        }
        TurnEvent::Retrying {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        } => writer.emit(&structured_output::NdjsonEvent::Retrying {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        }),
        TurnEvent::RateLimited {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        } => writer.emit(&structured_output::NdjsonEvent::RateLimited {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        }),
        TurnEvent::HookProgress { event, state } => {
            writer.emit(&structured_output::NdjsonEvent::Hook {
                event,
                state: state.as_str().to_string(),
                kind: None,
                content: None,
            })
        }
        TurnEvent::HookMessage {
            event,
            kind,
            content,
        } => writer.emit(&structured_output::NdjsonEvent::Hook {
            event,
            state: "message".to_string(),
            kind: Some(kind.as_str().to_string()),
            content: Some(content),
        }),
    }
}

/// Outcome of resolving a /resume argument against the saved sessions list.
enum ResumeLookup {
    Found(oxicode_session::Session),
    NotFound,
    Multiple(usize),
    SameAsCurrent,
}

/// Resolve a /resume argument to a concrete session.
///
/// Accepts: full UUID, UUID prefix, or substring of the session preview
/// (case-insensitive). Rejects matches on the current session and
/// ambiguous prefixes. Mirrors Claude Code's resume.tsx `call()` flow.
fn resolve_resume_arg(arg: &str, current_id: &str) -> ResumeLookup {
    // 1. Exact UUID — fastest path, also covers arg == current_id check.
    if let Ok(loaded) = oxicode_session::load_session(arg, None) {
        if loaded.id == current_id {
            return ResumeLookup::SameAsCurrent;
        }
        return ResumeLookup::Found(loaded);
    }

    let Ok(summaries) = oxicode_session::list_sessions(None) else {
        return ResumeLookup::NotFound;
    };
    let arg_lc = arg.to_lowercase();
    let mut matches: Vec<_> = summaries
        .into_iter()
        .filter(|s| s.id != current_id)
        .filter(|s| {
            s.id.to_lowercase().starts_with(&arg_lc)
                || s.preview
                    .as_deref()
                    .is_some_and(|p| p.to_lowercase().contains(&arg_lc))
        })
        .collect();

    match matches.len() {
        0 => ResumeLookup::NotFound,
        1 => {
            let s = matches.remove(0);
            oxicode_session::load_session(&s.id, None)
                .map_or(ResumeLookup::NotFound, ResumeLookup::Found)
        }
        n => ResumeLookup::Multiple(n),
    }
}

/// Translate a `TurnEvent` from the engine into a `CoreEvent` for the TUI.
fn translate_turn_event(te: oxicode_core::TurnEvent) -> CoreEvent {
    use oxicode_core::TurnEvent;
    match te {
        TurnEvent::TextDelta(t) => CoreEvent::TextDelta(t),
        TurnEvent::ThinkingDelta(t) => CoreEvent::ThinkingDelta(t),
        TurnEvent::TurnStart => CoreEvent::StreamStart,
        TurnEvent::TurnEnd => CoreEvent::StreamEnd,
        TurnEvent::ToolUseStart { id, name, input } => CoreEvent::ToolUseStart { id, name, input },
        TurnEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => CoreEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
        },
        TurnEvent::PermissionAsk {
            tool_name,
            input_summary,
            prompt,
            reply_tx,
        } => CoreEvent::PermissionAsk {
            tool_name,
            input_summary,
            prompt,
            reply_tx,
        },
        TurnEvent::Error(e) => CoreEvent::Error(e),
        TurnEvent::RateLimited {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        } => CoreEvent::RateLimited {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        },
        TurnEvent::Retrying {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        } => CoreEvent::Retrying {
            message,
            attempt,
            max_retries,
            retry_in_secs,
        },
        TurnEvent::HookProgress { event, state } => CoreEvent::HookProgress {
            event,
            state: state.as_str().to_string(),
        },
        TurnEvent::HookMessage {
            event,
            kind,
            content,
        } => CoreEvent::HookMessage {
            event,
            kind: kind.as_str().to_string(),
            content,
        },
    }
}

/// Run the interactive TUI.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)] // TUI event loop with many input handlers
async fn run_tui(
    engine: Arc<QueryEngine>,
    state_store: Arc<StateStore>,
    session: &mut Session,
    settings: &Settings,
    hook_manager: Arc<oxicode_hooks::HookManager>,
    file_state: Arc<oxicode_tools::file_state_tracker::FileStateTracker>,
    open_picker_on_start: bool,
    elicitation_rx: tokio::sync::mpsc::UnboundedReceiver<oxicode_mcp::ElicitationEnvelope>,
) -> Result<()> {
    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, core_rx) = mpsc::channel::<CoreEvent>(256);

    for msg in &session.messages {
        state_store.push_message(msg.clone());
    }

    // Build command registry early to extract metadata for TUI autocomplete.
    let command_registry = Arc::new(commands::default_registry());
    // Expose slash command count in shared state so TUI status bar can read it.
    state_store.update(|s| s.slash_command_count = command_registry.len());
    let meta_ctx = commands::CommandContext {
        state_store: state_store.clone(),
        model: settings.model.clone(),
        provider_name: "auto".into(),
        session_id: String::new(),
        command_registry: Arc::clone(&command_registry),
    };
    let slash_command_meta: Vec<oxicode_tui::SlashCommandMeta> = command_registry
        .all_commands_with_completions(&meta_ctx)
        .into_iter()
        .map(
            |(name, desc, arg_candidates)| oxicode_tui::SlashCommandMeta {
                name: name.to_string(),
                description: desc.to_string(),
                category: String::new(),
                arg_candidates,
            },
        )
        .collect();

    let mut app = App::new(&state_store, ui_tx, core_rx, slash_command_meta);
    app.set_elicitation_rx(elicitation_rx);

    // Rebuild click-to-open map for `[Image #N]` tags from resumed session data.
    app.hydrate_image_paths_from_session();

    // `oxicode -r` (no query) opens the picker as soon as the TUI is up.
    if open_picker_on_start {
        app.open_session_browser_on_startup();
    }

    // Wire editor mode from settings.
    if settings.editor_mode == "vim" || settings.features.vim_mode {
        app.set_vim_mode(true);
    }

    // Load user keybindings if file exists.
    let keybindings_path =
        oxicode_config::config_dir(settings.config_dir.as_deref()).join("keybindings.toml");
    app.load_keybindings(&keybindings_path);

    let engine_clone = engine.clone();
    let core_tx_clone = core_tx.clone();
    let state_store_clone = state_store.clone();

    // Cancel flag shared between TUI (sets on InterruptTurn) and engine (checks in loop).
    let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_flag_engine = cancel_flag.clone();

    // Give the TUI direct access to the cancel flag so Esc/Ctrl+C sets it
    // immediately, bypassing the channel queue that the engine task cannot
    // drain while blocked inside execute_turn_with_cancel.
    app.set_cancel_flag(cancel_flag.clone());

    let hook_manager_engine = hook_manager.clone();
    let file_state_engine = file_state.clone();

    // Engine task: owns conversation, calls execute_turn(), forwards events to TUI.
    let engine_handle = tokio::spawn(async move {
        let mut conversation = Conversation::new();
        let state = state_store_clone.current();
        for msg in &state.messages {
            conversation.push(msg.clone());
        }

        // command_registry was created above and moved here.

        while let Some(event) = ui_rx.recv().await {
            match event {
                UiEvent::UserInput { text, images } => {
                    let user_msg = if images.is_empty() {
                        Message::user(&text)
                    } else {
                        let img_blocks: Vec<oxicode_common::ContentBlock> = images
                            .iter()
                            .filter_map(|img| {
                                oxicode_tui::image_paste::encode_image_base64(&img.path).map(
                                    |b64| oxicode_common::ContentBlock::Image {
                                        source: oxicode_common::ImageSource {
                                            source_type: "base64".into(),
                                            media_type: Some("image/png".into()),
                                            data: Some(b64),
                                        },
                                    },
                                )
                            })
                            .collect();
                        Message::user_with_images(text, img_blocks)
                    };
                    // Push user message to state_store (for TUI rendering).
                    // execute_turn does NOT push user messages, only assistant/tool results.
                    state_store_clone.push_message(user_msg.clone());
                    conversation.push(user_msg);

                    // Create TurnEvent channel for this turn.
                    let (turn_tx, mut turn_rx) =
                        tokio::sync::mpsc::channel::<oxicode_core::TurnEvent>(256);

                    // Spawn forwarder: translates TurnEvent -> CoreEvent for TUI.
                    let core_tx_fwd = core_tx_clone.clone();
                    let forwarder = tokio::spawn(async move {
                        while let Some(te) = turn_rx.recv().await {
                            let _ = core_tx_fwd.send(translate_turn_event(te)).await;
                        }
                    });

                    // Run execute_turn in this task (owns conversation).
                    let result = engine_clone
                        .execute_turn_with_cancel(
                            &mut conversation,
                            Some(&turn_tx),
                            Some(&cancel_flag_engine),
                        )
                        .await;

                    // Drop sender to close forwarder, then wait for it.
                    drop(turn_tx);
                    let _ = forwarder.await;

                    // Clear any stale cancel flag so the NEXT turn doesn't
                    // auto-cancel from a leftover InterruptTurn event.
                    cancel_flag_engine.store(false, std::sync::atomic::Ordering::SeqCst);

                    // execute_turn already pushed messages to state_store and conversation.
                    match result {
                        Ok(_) => {
                            let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                        }
                        Err(e) => {
                            let _ = core_tx_clone.send(CoreEvent::Error(e.to_string())).await;
                            // Safety net: always send MessageComplete after Error so TUI
                            // resets is_turn_active even if Error handler has a bug.
                            let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                        }
                    }
                }
                UiEvent::SlashCommand { name, args } => {
                    // Handle /resume <arg> (or /continue <arg>) asynchronously —
                    // load the resolved session and hot-swap conversation + state
                    // so the TUI rerenders the old transcript without restart.
                    // Matches Claude Code's /resume entrypoint flow.
                    let is_resume_alias = name == "resume" || name == "continue";
                    if is_resume_alias && !args.trim().is_empty() {
                        let arg = args.trim().to_string();
                        let current_id = state_store_clone.current().session_id;
                        let resolved = resolve_resume_arg(&arg, &current_id);
                        match resolved {
                            ResumeLookup::Found(loaded) => {
                                let loaded_id = loaded.id.clone();

                                // 1. Persist current session's costs so they're not lost.
                                let cost_path = oxicode_state::cost_tracker::default_cost_path();
                                let current_tracker = state_store_clone.current().cost_tracker;
                                let _ = oxicode_state::cost_tracker::save_costs(
                                    &cost_path,
                                    &current_tracker,
                                );

                                // 2. SessionEnd hook for the old session before swap.
                                hook_manager_engine
                                    .fire_simple(oxicode_hooks::HookEvent::SessionEnd)
                                    .await;

                                // 3. Drop recorded mtimes — the resumed session's
                                //    tool history doesn't know about the current
                                //    session's reads and vice versa.
                                file_state_engine.clear();

                                // 4. Swap conversation + state in place.
                                conversation.replace_messages(loaded.messages.clone());
                                let target_tracker =
                                    oxicode_state::cost_tracker::load_costs(&cost_path, &loaded_id)
                                        .unwrap_or_else(|| {
                                            oxicode_state::cost_tracker::CostTracker::new(
                                                loaded_id.clone(),
                                            )
                                        });
                                state_store_clone.update(|s| {
                                    s.messages = loaded.messages;
                                    s.session_id.clone_from(&loaded_id);
                                    s.cost_tracker = target_tracker;
                                    s.total_usage = oxicode_common::Usage::default();
                                });

                                // 5. SessionStart hook with source=resume (Claude Code spec —
                                // no separate SessionLoad event).
                                let fields = oxicode_hooks::manager::HookFields {
                                    source: Some("resume".to_string()),
                                    ..Default::default()
                                };
                                hook_manager_engine
                                    .fire_with_fields(
                                        oxicode_hooks::HookEvent::SessionStart,
                                        fields,
                                    )
                                    .await;

                                // 6. Tell the TUI to rehydrate image paths + redraw.
                                let _ = core_tx_clone
                                    .send(CoreEvent::SessionResumed {
                                        session_id: loaded_id,
                                    })
                                    .await;
                                let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                            }
                            ResumeLookup::NotFound => {
                                let _ = core_tx_clone
                                    .send(CoreEvent::Error(format!("Session '{arg}' not found.")))
                                    .await;
                            }
                            ResumeLookup::Multiple(n) => {
                                let _ = core_tx_clone
                                    .send(CoreEvent::Error(format!(
                                        "Found {n} sessions matching '{arg}'. \
                                         Use /resume to pick one."
                                    )))
                                    .await;
                            }
                            ResumeLookup::SameAsCurrent => {
                                let _ = core_tx_clone
                                    .send(CoreEvent::Error("Already on that session.".into()))
                                    .await;
                            }
                        }
                        continue;
                    }

                    // Handle /compact asynchronously (needs LLM provider).
                    if name == "compact" {
                        let msg_count_before = conversation.len();
                        let messages = conversation.api_messages().to_vec();

                        if messages.len() < 3 {
                            let sys_msg = Message {
                                id: uuid::Uuid::new_v4().to_string(),
                                role: Role::Assistant,
                                content: vec![ContentBlock::Text {
                                    text: "Not enough messages to compact.".to_string(),
                                }],
                                model: None,
                                stop_reason: None,
                                created_at: chrono::Utc::now(),
                                usage: None,
                            };
                            state_store_clone.push_message(sys_msg);
                            let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                            continue;
                        }

                        let provider = engine_clone.provider_ref().clone();
                        let model = engine_clone.model();

                        match oxicode_context::AutoCompactor::compact(
                            &messages,
                            provider.as_ref(),
                            &model,
                        )
                        .await
                        {
                            Ok(summary_msg) => {
                                conversation.replace_messages(vec![summary_msg.clone()]);
                                state_store_clone.replace_messages(vec![summary_msg]);

                                let sys_msg = Message {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    role: Role::Assistant,
                                    content: vec![ContentBlock::Text {
                                        text: format!(
                                            "Context compacted: {msg_count_before} messages → 1 summary.",
                                        ),
                                    }],
                                    model: None,
                                    stop_reason: None,
                                    created_at: chrono::Utc::now(),
                                    usage: None,
                                };
                                state_store_clone.push_message(sys_msg);
                                let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                            }
                            Err(e) => {
                                let _ = core_tx_clone
                                    .send(CoreEvent::Error(format!("Compact failed: {e}")))
                                    .await;
                            }
                        }
                        continue;
                    }

                    // Handle /switch-branch <branch-uuid> — hot-swap conversation
                    // to the selected branch inside the current session tree.
                    if name == "switch-branch" {
                        let arg = args.trim().to_string();
                        match arg.parse::<uuid::Uuid>() {
                            Err(_) => {
                                let _ = core_tx_clone
                                    .send(CoreEvent::Error(format!(
                                        "Invalid branch ID '{arg}': must be a UUID."
                                    )))
                                    .await;
                            }
                            Ok(branch_id) => {
                                let session_id = state_store_clone.current().session_id;
                                match oxicode_session::branch::SessionTree::load_or_migrate(
                                    &session_id,
                                    None,
                                ) {
                                    Err(e) => {
                                        let _ = core_tx_clone
                                            .send(CoreEvent::Error(format!(
                                                "Cannot load session tree: {e}"
                                            )))
                                            .await;
                                    }
                                    Ok(mut tree) => {
                                        if let Err(e) = tree.switch(branch_id) {
                                            let _ = core_tx_clone
                                                .send(CoreEvent::Error(format!(
                                                    "Cannot switch branch: {e}"
                                                )))
                                                .await;
                                        } else {
                                            // Persist the updated current pointer.
                                            if let Err(e) = tree.save(None) {
                                                let _ = core_tx_clone
                                                    .send(CoreEvent::Error(format!(
                                                        "Branch switch failed to persist: {e}"
                                                    )))
                                                    .await;
                                                continue;
                                            }
                                            let branch = tree
                                                .branches
                                                .get(&branch_id)
                                                .expect("just switched to it");
                                            let new_messages = branch.messages.clone();
                                            let title = branch.title.clone();
                                            // Hot-swap conversation + state.
                                            conversation.replace_messages(new_messages.clone());
                                            state_store_clone.update(|s| {
                                                s.messages = new_messages;
                                            });
                                            // Notify TUI to rehydrate + redraw.
                                            let _ = core_tx_clone
                                                .send(CoreEvent::SessionResumed {
                                                    session_id: session_id.clone(),
                                                })
                                                .await;
                                            let sys_msg = Message {
                                                id: uuid::Uuid::new_v4().to_string(),
                                                role: Role::Assistant,
                                                content: vec![ContentBlock::Text {
                                                    text: format!("Switched to branch: {title}"),
                                                }],
                                                model: None,
                                                stop_reason: None,
                                                created_at: chrono::Utc::now(),
                                                usage: None,
                                            };
                                            state_store_clone.push_message(sys_msg);
                                            let _ = core_tx_clone
                                                .send(CoreEvent::MessageComplete)
                                                .await;
                                        }
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    // Handle /rename-branch <branch-uuid> <new-title>
                    if name == "rename-branch" {
                        let mut parts = args.trim().splitn(2, ' ');
                        let id_str = parts.next().unwrap_or("").trim();
                        let new_title = parts.next().unwrap_or("").trim();
                        match (id_str.parse::<uuid::Uuid>(), new_title.is_empty()) {
                            (Err(_), _) => {
                                let _ = core_tx_clone
                                    .send(CoreEvent::Error(format!(
                                        "Invalid branch ID '{id_str}': must be a UUID."
                                    )))
                                    .await;
                            }
                            (_, true) => {
                                let _ = core_tx_clone
                                    .send(CoreEvent::Error(
                                        "rename-branch: new title cannot be empty.".into(),
                                    ))
                                    .await;
                            }
                            (Ok(branch_id), false) => {
                                let session_id = state_store_clone.current().session_id;
                                match oxicode_session::branch::SessionTree::load_or_migrate(
                                    &session_id,
                                    None,
                                ) {
                                    Err(e) => {
                                        let _ = core_tx_clone
                                            .send(CoreEvent::Error(format!(
                                                "Cannot load session tree: {e}"
                                            )))
                                            .await;
                                    }
                                    Ok(mut tree) => {
                                        if let Some(branch) = tree.branches.get_mut(&branch_id) {
                                            branch.title = new_title.to_string();
                                            match tree.save(None) {
                                                Ok(()) => {
                                                    let sys_msg = Message {
                                                        id: uuid::Uuid::new_v4().to_string(),
                                                        role: Role::Assistant,
                                                        content: vec![ContentBlock::Text {
                                                            text: format!(
                                                                "Branch renamed to \"{new_title}\"."
                                                            ),
                                                        }],
                                                        model: None,
                                                        stop_reason: None,
                                                        created_at: chrono::Utc::now(),
                                                        usage: None,
                                                    };
                                                    state_store_clone.push_message(sys_msg);
                                                    let _ = core_tx_clone
                                                        .send(CoreEvent::MessageComplete)
                                                        .await;
                                                }
                                                Err(e) => {
                                                    let _ = core_tx_clone
                                                        .send(CoreEvent::Error(format!(
                                                            "Rename failed to persist: {e}"
                                                        )))
                                                        .await;
                                                }
                                            }
                                        } else {
                                            let _ = core_tx_clone
                                                .send(CoreEvent::Error(format!(
                                                    "Branch not found: {branch_id}"
                                                )))
                                                .await;
                                        }
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    // Handle /delete-branch <branch-uuid>
                    if name == "delete-branch" {
                        let arg = args.trim().to_string();
                        match arg.parse::<uuid::Uuid>() {
                            Err(_) => {
                                let _ = core_tx_clone
                                    .send(CoreEvent::Error(format!(
                                        "Invalid branch ID '{arg}': must be a UUID."
                                    )))
                                    .await;
                            }
                            Ok(branch_id) => {
                                let session_id = state_store_clone.current().session_id;
                                match oxicode_session::branch::SessionTree::load_or_migrate(
                                    &session_id,
                                    None,
                                ) {
                                    Err(e) => {
                                        let _ = core_tx_clone
                                            .send(CoreEvent::Error(format!(
                                                "Cannot load session tree: {e}"
                                            )))
                                            .await;
                                    }
                                    Ok(mut tree) => match tree.delete_branch(branch_id, None) {
                                        Err(e) => {
                                            let _ = core_tx_clone
                                                .send(CoreEvent::Error(format!(
                                                    "Cannot delete branch: {e}"
                                                )))
                                                .await;
                                        }
                                        Ok(()) => match tree.save(None) {
                                            Ok(()) => {
                                                let sys_msg = Message {
                                                    id: uuid::Uuid::new_v4().to_string(),
                                                    role: Role::Assistant,
                                                    content: vec![ContentBlock::Text {
                                                        text: format!(
                                                            "Branch {branch_id} deleted."
                                                        ),
                                                    }],
                                                    model: None,
                                                    stop_reason: None,
                                                    created_at: chrono::Utc::now(),
                                                    usage: None,
                                                };
                                                state_store_clone.push_message(sys_msg);
                                                let _ = core_tx_clone
                                                    .send(CoreEvent::MessageComplete)
                                                    .await;
                                            }
                                            Err(e) => {
                                                let _ = core_tx_clone
                                                        .send(CoreEvent::Error(format!(
                                                            "Delete succeeded in memory but failed to persist: {e}"
                                                        )))
                                                        .await;
                                            }
                                        },
                                    },
                                }
                            }
                        }
                        continue;
                    }

                    // ─── Async slash command dispatch (Phases 2-5) ───
                    // The sync command registry can't reach async tools because
                    // SlashCommand::execute is sync. These arms intercept before
                    // the registry fallthrough and dispatch to real backends.

                    // /cron [list|create <expr> <command>|delete <id>]
                    if name == "cron" {
                        let args_trimmed = args.trim();
                        let (sub, rest) =
                            args_trimmed.split_once(' ').unwrap_or((args_trimmed, ""));
                        let dispatch = match sub {
                            "" | "list" => Some(("cron_list", serde_json::json!({}))),
                            "create" => {
                                let rest_trim = rest.trim();
                                let (expr, command) = rest_trim
                                    .split_once(' ')
                                    .map_or((rest_trim, ""), |(e, c)| (e.trim(), c.trim()));
                                if expr.is_empty() || command.is_empty() {
                                    let _ = core_tx_clone
                                        .send(CoreEvent::Error(
                                            "Usage: /cron create <cron-expr> <command>".into(),
                                        ))
                                        .await;
                                    None
                                } else {
                                    Some((
                                        "cron_create",
                                        serde_json::json!({
                                            "cron": expr,
                                            "command": command,
                                        }),
                                    ))
                                }
                            }
                            "delete" => {
                                let id = rest.trim();
                                if id.is_empty() {
                                    let _ = core_tx_clone
                                        .send(CoreEvent::Error("Usage: /cron delete <id>".into()))
                                        .await;
                                    None
                                } else {
                                    Some(("cron_delete", serde_json::json!({ "id": id })))
                                }
                            }
                            other => {
                                let _ = core_tx_clone
                                    .send(CoreEvent::Error(format!(
                                        "Unknown /cron subcommand: {other}. Use list/create/delete.",
                                    )))
                                    .await;
                                None
                            }
                        };
                        if let Some((tool_name, input)) = dispatch {
                            let registry = engine_clone.tool_registry().clone();
                            let ctx = engine_clone.tool_context().clone();
                            match registry.execute(tool_name, input, &ctx).await {
                                Ok(res) => {
                                    let sys_msg = Message {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        role: Role::Assistant,
                                        content: vec![ContentBlock::Text { text: res.content }],
                                        model: None,
                                        stop_reason: None,
                                        created_at: chrono::Utc::now(),
                                        usage: None,
                                    };
                                    state_store_clone.push_message(sys_msg);
                                    let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                                }
                                Err(e) => {
                                    let _ = core_tx_clone
                                        .send(CoreEvent::Error(format!("/cron failed: {e}")))
                                        .await;
                                }
                            }
                        }
                        continue;
                    }

                    // /schedule <cron-expr> <command> — friendly alias for cron_create.
                    if name == "schedule" {
                        let args_trimmed = args.trim();
                        if args_trimmed.is_empty() {
                            let sys_msg = Message {
                                id: uuid::Uuid::new_v4().to_string(),
                                role: Role::Assistant,
                                content: vec![ContentBlock::Text {
                                    text: "Usage: /schedule <cron-expr> <command>".into(),
                                }],
                                model: None,
                                stop_reason: None,
                                created_at: chrono::Utc::now(),
                                usage: None,
                            };
                            state_store_clone.push_message(sys_msg);
                            let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                            continue;
                        }
                        let (expr, command) = args_trimmed
                            .split_once(' ')
                            .map_or((args_trimmed, ""), |(e, c)| (e.trim(), c.trim()));
                        if expr.is_empty() || command.is_empty() {
                            let _ = core_tx_clone
                                .send(CoreEvent::Error(
                                    "/schedule requires <cron-expr> and <command>".into(),
                                ))
                                .await;
                            continue;
                        }
                        let registry = engine_clone.tool_registry().clone();
                        let ctx = engine_clone.tool_context().clone();
                        let input = serde_json::json!({
                            "cron": expr,
                            "command": command,
                            "description": "scheduled via /schedule",
                        });
                        match registry.execute("cron_create", input, &ctx).await {
                            Ok(res) => {
                                let sys_msg = Message {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    role: Role::Assistant,
                                    content: vec![ContentBlock::Text { text: res.content }],
                                    model: None,
                                    stop_reason: None,
                                    created_at: chrono::Utc::now(),
                                    usage: None,
                                };
                                state_store_clone.push_message(sys_msg);
                                let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                            }
                            Err(e) => {
                                let _ = core_tx_clone
                                    .send(CoreEvent::Error(format!("/schedule failed: {e}")))
                                    .await;
                            }
                        }
                        continue;
                    }

                    // /worktree create <branch> [path] — list/exit fall through to sync.
                    // (Underlying enter_worktree tool requires `branch`; path optional.)
                    if name == "worktree" && args.trim_start().starts_with("create") {
                        let after = args.trim_start()[6..].trim();
                        if after.is_empty() {
                            let _ = core_tx_clone
                                .send(CoreEvent::Error(
                                    "Usage: /worktree create <branch> [path]".into(),
                                ))
                                .await;
                            continue;
                        }
                        let (branch, path) = after
                            .split_once(' ')
                            .map_or((after, ""), |(b, p)| (b.trim(), p.trim()));
                        let mut input = serde_json::json!({ "branch": branch });
                        if !path.is_empty() {
                            input["path"] = serde_json::Value::String(path.into());
                        }
                        let registry = engine_clone.tool_registry().clone();
                        let ctx = engine_clone.tool_context().clone();
                        match registry.execute("enter_worktree", input, &ctx).await {
                            Ok(res) => {
                                let sys_msg = Message {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    role: Role::Assistant,
                                    content: vec![ContentBlock::Text { text: res.content }],
                                    model: None,
                                    stop_reason: None,
                                    created_at: chrono::Utc::now(),
                                    usage: None,
                                };
                                state_store_clone.push_message(sys_msg);
                                let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                            }
                            Err(e) => {
                                let _ = core_tx_clone
                                    .send(CoreEvent::Error(format!("/worktree create failed: {e}")))
                                    .await;
                            }
                        }
                        continue;
                    }

                    // /retry — replay the last user message by truncating back to
                    // the most recent Role::User and re-running a turn.
                    if name == "retry" {
                        let last_user_idx = conversation
                            .api_messages()
                            .iter()
                            .rposition(|m| m.role == Role::User);
                        let Some(idx) = last_user_idx else {
                            let _ = core_tx_clone
                                .send(CoreEvent::Error("No previous message to retry.".into()))
                                .await;
                            continue;
                        };
                        let keep = idx + 1;
                        conversation.messages.truncate(keep);
                        state_store_clone.update(|s| s.messages.truncate(keep));

                        // Re-run a turn with the existing tail user message.
                        let (turn_tx, mut turn_rx) =
                            tokio::sync::mpsc::channel::<oxicode_core::TurnEvent>(256);
                        let core_tx_fwd = core_tx_clone.clone();
                        let forwarder = tokio::spawn(async move {
                            while let Some(te) = turn_rx.recv().await {
                                let _ = core_tx_fwd.send(translate_turn_event(te)).await;
                            }
                        });
                        let result = engine_clone
                            .execute_turn_with_cancel(
                                &mut conversation,
                                Some(&turn_tx),
                                Some(&cancel_flag_engine),
                            )
                            .await;
                        drop(turn_tx);
                        let _ = forwarder.await;
                        cancel_flag_engine.store(false, std::sync::atomic::Ordering::SeqCst);
                        match result {
                            Ok(_) => {
                                let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                            }
                            Err(e) => {
                                let _ = core_tx_clone.send(CoreEvent::Error(e.to_string())).await;
                                let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                            }
                        }
                        continue;
                    }

                    let current = state_store_clone.current();
                    let command_ctx = commands::CommandContext {
                        state_store: state_store_clone.clone(),
                        model: current.current_model.clone(),
                        provider_name: "auto".into(),
                        session_id: current.session_id.clone(),
                        command_registry: Arc::clone(&command_registry),
                    };
                    let input_str = format!("/{name} {args}");
                    match command_registry.execute(&input_str, &command_ctx) {
                        Some(commands::CommandOutput::Message(msg)) => {
                            let sys_msg = Message {
                                id: uuid::Uuid::new_v4().to_string(),
                                role: Role::Assistant,
                                content: vec![ContentBlock::Text { text: msg }],
                                model: None,
                                stop_reason: None,
                                created_at: chrono::Utc::now(),
                                usage: None,
                            };
                            state_store_clone.push_message(sys_msg);
                            let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                        }
                        Some(commands::CommandOutput::Quit) => break,
                        Some(commands::CommandOutput::Error(msg)) => {
                            let _ = core_tx_clone.send(CoreEvent::Error(msg)).await;
                        }
                        Some(commands::CommandOutput::Silent) => {}
                        Some(commands::CommandOutput::SwitchModel(new_model)) => {
                            // Validate model via ProviderRouter before switching.
                            let router = ProviderRouter::from_env();
                            match router.resolve(&new_model) {
                                Ok(resolved) => {
                                    engine_clone.set_model(resolved.model.clone());
                                    state_store_clone.update(|s| {
                                        s.current_model.clone_from(&resolved.model);
                                    });
                                    let sys_msg = Message {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        role: Role::Assistant,
                                        content: vec![ContentBlock::Text {
                                            text: format!("Switched to model: {}", resolved.model),
                                        }],
                                        model: None,
                                        stop_reason: None,
                                        created_at: chrono::Utc::now(),
                                        usage: None,
                                    };
                                    state_store_clone.push_message(sys_msg);
                                    let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                                }
                                Err(e) => {
                                    let _ = core_tx_clone
                                        .send(CoreEvent::Error(format!("Cannot switch model: {e}")))
                                        .await;
                                }
                            }
                        }
                        None => {
                            let _ = core_tx_clone
                                .send(CoreEvent::Error(format!("Unknown command: /{name}")))
                                .await;
                        }
                    }
                }
                UiEvent::SettingsSaved {
                    model,
                    permission_mode,
                } => {
                    // Propagate saved settings into the live StateStore so the
                    // status bar and other reactive components update immediately.
                    state_store_clone.update(|s| {
                        s.current_model.clone_from(&model);
                        s.permission_mode.clone_from(&permission_mode);
                    });
                    // Update the engine's active model if it changed.
                    engine_clone.set_model(model);
                }
                UiEvent::Quit => break,
                // InterruptTurn is a no-op: the TUI sets cancel_flag directly via the
                // shared Arc<AtomicBool> (signal_interrupt), so the engine sees it
                // immediately. Processing this queued event would re-arm the flag AFTER
                // it was cleared, poisoning the next turn.
                _ => {}
            }
        }
    });

    app.run().await?;

    // Persist to whichever session the user ended on. After `/resume <id>` the
    // TUI hot-swaps `state.session_id` to the resumed id — if we kept writing
    // to the original `session.id` here, the resumed transcript would clobber
    // the original session's file and both records would be corrupted.
    let state = state_store.current();
    if session.id != state.session_id {
        session.id = state.session_id;
    }
    session.messages = state.messages;
    // Persist the current working_dirs set so `--session <id>` restores them.
    session.set_working_dirs(state.working_dirs.clone());
    session.updated_at = chrono::Utc::now();
    oxicode_session::save_session(session, None)?;

    // Graceful shutdown: wait for engine task to finish (ui_tx dropped → ui_rx returns None).
    // Fallback to abort after timeout.
    match tokio::time::timeout(std::time::Duration::from_secs(5), engine_handle).await {
        Ok(_) => {}
        Err(_) => {
            tracing::warn!("Engine task didn't stop in 5s, aborting");
        }
    }

    Ok(())
}

/// Run as a subagent: read AgentConfig from stdin, execute, write result to stdout.
async fn run_agent_mode(agent_id: &str) -> Result<()> {
    use std::io::Read;

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    let config: oxicode_agents::AgentConfig = serde_json::from_str(&input)
        .map_err(|e| anyhow::anyhow!("Failed to parse agent config: {e}"))?;

    tracing::info!(agent_id = %agent_id, name = %config.name, "agent mode started");

    let settings = oxicode_config::load_settings(None);
    let router = ProviderRouter::from_env();
    let model = config.model.clone();
    let resolved = match router.resolve(&model) {
        Ok(r) => r,
        Err(e) => {
            let result = serde_json::json!({
                "agent_id": agent_id,
                "output": "",
                "is_error": true,
                "error": format!("Provider resolution failed: {e}")
            });
            println!("{result}");
            return Ok(());
        }
    };

    let cwd = config.working_dir.clone();
    let (global_md, project_md) = oxicode_config::load_claude_md(&cwd);
    let system_prompt = oxicode_core::system_prompt::assemble_system_prompt(
        global_md.as_deref(),
        project_md.as_deref(),
        None,
        None,
    );

    let state_store = Arc::new(StateStore::new(AppState::default()));

    // Enforce agent-type tool whitelist: remove tools not in the allowed list.
    let mut registry = oxicode_tools::default_registry();
    if let Some(ref whitelist) = config.allowed_tools {
        tracing::info!(
            agent_id = %agent_id,
            allowed = ?whitelist,
            "Enforcing tool whitelist for agent type {:?}",
            config.agent_type
        );
        registry.retain(|name| whitelist.iter().any(|w| w == name));
    }
    let tool_registry = Arc::new(registry);
    let permission_mode = PermissionMode::parse(&config.permission_mode);
    let permission_pipeline = Arc::new(PermissionPipeline::new(permission_mode, vec![]));

    // Child agent gets its own HookManager from settings. Tag it with
    // agent_id/agent_type so every hook payload fired from this subagent
    // process carries the subagent identity (Claude Code parity).
    let mut child_hook_manager = HookManager::from_settings();
    child_hook_manager.set_subagent(
        config.name.clone(),
        config.agent_type.map(|t| t.as_str().to_string()),
    );
    let hook_manager = Arc::new(child_hook_manager);

    let tool_context = ToolContext {
        working_dir: cwd,
        extra_working_dirs: Vec::new(), // agent mode inherits no extra dirs at spawn
        file_state: Arc::new(oxicode_tools::file_state_tracker::FileStateTracker::default()),
        task_manager: Arc::new(std::sync::Mutex::new(oxicode_tasks::TaskManager::default())),
        task_abort_handles: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        mcp_manager: Arc::new(oxicode_mcp::McpServerManager::new()),
        skill_executor: None, // Agent mode doesn't initialize skills
        team_manager: Arc::new(std::sync::Mutex::new(oxicode_agents::TeamManager::new())),
        bash_processes: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        hook_manager: Some(hook_manager),
        permission_mode,
    };

    let engine = Arc::new(QueryEngine::new(
        resolved.provider,
        state_store,
        tool_registry,
        permission_pipeline,
        tool_context,
        resolved.model,
        settings.max_tokens,
        system_prompt,
    ));

    let started = std::time::Instant::now();
    let mut conversation = Conversation::new();
    conversation.push(Message::user(&config.prompt));

    let (output, is_error) = match engine.execute_turn(&mut conversation, None).await {
        Ok(msg) => (msg.text(), false),
        Err(e) => (e.to_string(), true),
    };

    let result = serde_json::json!({
        "agent_id": agent_id,
        "output": output,
        "is_error": is_error,
        "duration_ms": started.elapsed().as_millis() as u64
    });
    println!("{result}");
    Ok(())
}

/// Run OxiCode as an MCP server over stdio.
///
/// Exposes a `read_file` demo tool; full tool wiring is future work.
async fn run_mcp_server_mode() -> Result<()> {
    use oxicode_mcp::server::OxiMcpServerBuilder;
    use oxicode_mcp::{McpContent, McpToolResult};
    use std::sync::Arc;

    let read_file_handler: oxicode_mcp::server::AsyncToolHandler =
        Arc::new(|args: serde_json::Value| {
            Box::pin(async move {
                let path = args
                    .get("file_path")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                match tokio::fs::read_to_string(path).await {
                    Ok(content) => McpToolResult::success(vec![McpContent::text(content)]),
                    Err(e) => McpToolResult::error(vec![McpContent::text(format!(
                        "Failed to read file: {e}"
                    ))]),
                }
            })
        });

    let server = OxiMcpServerBuilder::new()
        .add_tool(
            "read_file",
            "Read a file from the filesystem",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "Absolute path to file" }
                },
                "required": ["file_path"]
            }),
            read_file_handler,
        )
        .build();

    oxicode_mcp::run_mcp_server(server).await?;
    Ok(())
}

/// Switch-branch handler helper: load the tree, call `switch`, persist, return
/// the new messages. Extracted so the logic can be unit-tested without the
/// full async engine harness.
#[cfg(test)]
fn do_switch_branch(
    session_id: &str,
    branch_id: uuid::Uuid,
    config_dir_override: Option<&std::path::Path>,
) -> Result<(Vec<oxicode_common::Message>, String), String> {
    let mut tree =
        oxicode_session::branch::SessionTree::load_or_migrate(session_id, config_dir_override)
            .map_err(|e| format!("Cannot load session tree: {e}"))?;
    tree.switch(branch_id)
        .map_err(|e| format!("Cannot switch branch: {e}"))?;
    tree.save(config_dir_override)
        .map_err(|e| format!("Branch switch failed to persist: {e}"))?;
    let branch = tree.branches.get(&branch_id).expect("just switched to it");
    Ok((branch.messages.clone(), branch.title.clone()))
}

#[cfg(test)]
mod resume_lookup_tests {
    use super::*;

    fn save(id: &str, preview: &str) {
        use oxicode_common::{ContentBlock, Message, Role};
        let mut s = oxicode_session::Session::new("test-model");
        s.id = id.into();
        s.messages.push(Message {
            id: "m".into(),
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: preview.into(),
            }],
            model: None,
            stop_reason: None,
            created_at: chrono::Utc::now(),
            usage: None,
        });
        oxicode_session::save_session(&s, None).unwrap();
    }

    /// Single test that exercises every resolve_resume_arg outcome.
    ///
    /// Combined into one test because resolve_resume_arg reads `$HOME` via
    /// `dirs::home_dir()`; parallel tests that mutate HOME race with each
    /// other. Keeping this sequential also keeps the saved-session fixture
    /// deterministic.
    #[test]
    fn test_resolve_resume_arg_all_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("HOME");
        std::env::set_var("HOME", tmp.path());
        let result = std::panic::catch_unwind(|| {
            // Seed four sessions: full-uuid, prefix, preview-match, ambiguous pair.
            save("aaaaaaaa-1111-2222-3333-444444444444", "hello world");
            save("bbbbbbbb-aaaa-bbbb-cccc-dddddddddddd", "bug fix");
            save("cccccccc-1111-2222-3333-444444444444", "migrate db");
            save("dddddddd-1111-2222-3333-444444444444", "current session");
            save("eeee1111-1111-2222-3333-444444444444", "debug x");
            save("eeee2222-1111-2222-3333-444444444444", "debug y");

            // Full UUID — Found.
            assert!(matches!(
                resolve_resume_arg("aaaaaaaa-1111-2222-3333-444444444444", "other"),
                ResumeLookup::Found(_)
            ));
            // UUID prefix — Found.
            assert!(matches!(
                resolve_resume_arg("bbbb", "other"),
                ResumeLookup::Found(_)
            ));
            // Preview substring — Found.
            assert!(matches!(
                resolve_resume_arg("migrate", "other"),
                ResumeLookup::Found(_)
            ));
            // Requesting the current session — SameAsCurrent.
            assert!(matches!(
                resolve_resume_arg(
                    "dddddddd-1111-2222-3333-444444444444",
                    "dddddddd-1111-2222-3333-444444444444"
                ),
                ResumeLookup::SameAsCurrent
            ));
            // Ambiguous prefix — Multiple(2).
            assert!(matches!(
                resolve_resume_arg("eeee", "other"),
                ResumeLookup::Multiple(2)
            ));
            // No match — NotFound.
            assert!(matches!(
                resolve_resume_arg("no-such-thing", "other"),
                ResumeLookup::NotFound
            ));
        });
        match prev {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        result.unwrap();
    }
}

#[cfg(test)]
mod switch_branch_tests {
    use super::*;
    use oxicode_common::{ContentBlock, Message, Role};
    use oxicode_session::branch::SessionTree;

    fn push_msg(branch: &mut oxicode_session::branch::SessionBranch, text: &str) {
        branch.messages.push(Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            model: None,
            stop_reason: None,
            created_at: chrono::Utc::now(),
            usage: None,
        });
    }

    /// do_switch_branch with a valid branch returns messages and does not error.
    #[test]
    fn switch_valid_branch_returns_messages() {
        let tmp = tempfile::tempdir().unwrap();
        let config = Some(tmp.path());

        // Build a tree: root + one fork.
        let mut tree = SessionTree::new("model-x");
        let root_id = tree.current;
        {
            let root = tree.branches.get_mut(&root_id).unwrap();
            push_msg(root, "root msg 0");
            push_msg(root, "root msg 1");
        }
        let child_id = tree.fork(root_id, 1, "alt".to_string()).unwrap();
        {
            let child = tree.branches.get_mut(&child_id).unwrap();
            push_msg(child, "child msg 2");
        }
        // Save tree using the temp dir as config root.
        tree.save(config).unwrap();

        let session_id = tree.session_id.to_string();

        // Switch to root (child was current after fork).
        let (msgs, title) = do_switch_branch(&session_id, root_id, config).expect("should succeed");
        assert_eq!(title, "main");
        assert_eq!(msgs.len(), 2);

        // Switch back to child.
        let (msgs2, title2) =
            do_switch_branch(&session_id, child_id, config).expect("should succeed");
        assert_eq!(title2, "alt");
        // Child was forked at turn 1 (2 msgs) + 1 extra = 3.
        assert_eq!(msgs2.len(), 3);
    }

    /// do_switch_branch with an unknown UUID returns an error (no panic).
    #[test]
    fn switch_unknown_branch_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let config = Some(tmp.path());

        let tree = SessionTree::new("model-x");
        tree.save(config).unwrap();
        let session_id = tree.session_id.to_string();

        let unknown = uuid::Uuid::new_v4();
        let result = do_switch_branch(&session_id, unknown, config);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("not found") || msg.contains("Branch"),
            "unexpected error: {msg}"
        );
    }
}

#[cfg(test)]
mod session_id_validation_tests {
    use super::validate_session_id;

    #[test]
    fn valid_session_id_is_accepted() {
        assert!(validate_session_id("a1b2c3d4-e5f6-7890-abcd-ef1234567890").is_ok());
        assert!(validate_session_id("my-session-123").is_ok());
        assert!(validate_session_id("session").is_ok());
    }

    #[test]
    fn session_id_with_forward_slash_is_rejected() {
        let result = validate_session_id("../../etc/passwd");
        assert!(result.is_err(), "path traversal must be rejected");
        let msg = result.unwrap_err();
        assert!(msg.contains(".."), "error should mention the bad id: {msg}");
    }

    #[test]
    fn session_id_with_backslash_is_rejected() {
        let result = validate_session_id("..\\windows\\system32");
        assert!(result.is_err(), "backslash must be rejected");
    }

    #[test]
    fn session_id_with_dotdot_only_is_rejected() {
        assert!(validate_session_id("..").is_err());
        assert!(validate_session_id("a/../b").is_err());
    }

    #[test]
    fn session_id_with_embedded_slash_is_rejected() {
        assert!(validate_session_id("legit/attack").is_err());
    }
}
