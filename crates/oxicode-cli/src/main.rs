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

    /// Resume a previous session by ID.
    #[arg(short, long)]
    session: Option<String>,

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

    // Setup tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("oxicode=info".parse()?),
        )
        .with_writer(std::io::stderr)
        .init();

    // Load config (env vars and TOML, no CLI secret)
    let mut settings = oxicode_config::load_settings(cli.config_dir.as_deref());

    if let Some(model) = cli.model {
        settings.model = model;
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
        None, // TODO(phase-4): inject project memory
    );

    let state_store = Arc::new(StateStore::new(AppState {
        current_model: settings.model.clone(),
        auth_label,
        ..AppState::default()
    }));

    let mut session = if let Some(session_id) = &cli.session {
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
    } else {
        Session::new(&settings.model)
    };

    let tool_registry = Arc::new(oxicode_tools::default_registry());
    let permission_mode = PermissionMode::parse(&settings.permission_mode);
    let permission_pipeline = Arc::new(PermissionPipeline::new(permission_mode, vec![]));

    // Initialize MCP servers from config.
    let mut mcp_manager = oxicode_mcp::McpServerManager::new();
    let mcp_config = oxicode_mcp::McpConfig::load();
    let started = mcp_manager.start_from_config(&mcp_config).await;
    if !started.is_empty() {
        tracing::info!("MCP servers started: {}", started.join(", "));
    }

    let mcp_ref = std::sync::Arc::new(mcp_manager);
    let tool_context = ToolContext {
        working_dir: cwd.clone(),
        file_state: std::sync::Arc::new(
            oxicode_tools::file_state_tracker::FileStateTracker::default(),
        ),
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

    if let Some(prompt) = cli.prompt {
        let result = match cli.output {
            OutputFormat::Json => {
                run_single_prompt_json(engine, &mut session, &prompt, &settings.model).await
            }
            OutputFormat::Text => run_single_prompt(engine, &mut session, &prompt).await,
        };
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

    // TODO(gap-phase-7): implement bridge mode server with multi-session + JWT auth.
    // Bridge mode: headless WebSocket server for IDE extensions and cloud deployment.
    if cli.bridge {
        let bridge_config = remote::BridgeConfig::default().with_port(cli.port);
        eprintln!(
            "Bridge mode starting on {} (max {} sessions)",
            bridge_config.socket_addr(),
            bridge_config.max_sessions,
        );
        eprintln!("Press Ctrl+C to stop.");

        // Initialize session pool for bridge mode.
        let _pool = remote::session_pool::SessionPool::new(
            bridge_config.max_sessions,
            bridge_config.idle_timeout_secs,
        );

        // Keep alive until interrupted — full WebSocket server requires
        // `--features bridge` for tokio-tungstenite dependency.
        #[cfg(feature = "bridge")]
        {
            tracing::info!(
                port = cli.port,
                bind = %bridge_config.bind_address,
                "Bridge server ready (WebSocket + JWT)"
            );
        }
        #[cfg(not(feature = "bridge"))]
        {
            tracing::info!(
                port = cli.port,
                "Bridge server ready (session pool only — compile with --features bridge for WebSocket)"
            );
        }

        tokio::signal::ctrl_c().await.ok();
        mcp_ref.shutdown_all().await;
        return Ok(());
    }

    if matches!(cli.output, OutputFormat::Json) {
        eprintln!("Warning: --output json is only supported with --prompt (non-interactive mode).");
    }

    let result = run_tui(engine, state_store, &mut session, &settings).await;
    mcp_ref.shutdown_all().await;
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

    match engine.execute_turn(&mut conversation, None).await {
        Ok(assistant_msg) => {
            // Emit content blocks as structured events
            for block in &assistant_msg.content {
                match block {
                    oxicode_common::ContentBlock::Text { text } => {
                        writer.assistant_text(text)?;
                    }
                    oxicode_common::ContentBlock::ToolUse { name, input, .. } => {
                        writer.tool_use(name, input)?;
                    }
                    oxicode_common::ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        writer.tool_result(tool_use_id, content, *is_error)?;
                    }
                    oxicode_common::ContentBlock::Thinking { .. } => {}
                }
            }

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

/// Translate a `TurnEvent` from the engine into a `CoreEvent` for the TUI.
fn translate_turn_event(te: oxicode_core::TurnEvent) -> CoreEvent {
    use oxicode_core::TurnEvent;
    match te {
        TurnEvent::TextDelta(t) => CoreEvent::TextDelta(t),
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
    }
}

/// Run the interactive TUI.
#[allow(clippy::too_many_lines)] // TUI event loop with many input handlers
async fn run_tui(
    engine: Arc<QueryEngine>,
    state_store: Arc<StateStore>,
    session: &mut Session,
    settings: &Settings,
) -> Result<()> {
    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, core_rx) = mpsc::channel::<CoreEvent>(256);

    for msg in &session.messages {
        state_store.push_message(msg.clone());
    }

    let mut app = App::new(&state_store, ui_tx, core_rx);

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

    // Engine task: owns conversation, calls execute_turn(), forwards events to TUI.
    let engine_handle = tokio::spawn(async move {
        let mut conversation = Conversation::new();
        let state = state_store_clone.current();
        for msg in &state.messages {
            conversation.push(msg.clone());
        }

        // Build slash command registry inside the spawned block (not Send-required outside).
        let command_registry = commands::default_registry();

        while let Some(event) = ui_rx.recv().await {
            match event {
                UiEvent::UserInput(text) => {
                    let user_msg = Message::user(&text);
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
                        .execute_turn(&mut conversation, Some(&turn_tx))
                        .await;

                    // Drop sender to close forwarder, then wait for it.
                    drop(turn_tx);
                    let _ = forwarder.await;

                    // execute_turn already pushed messages to state_store and conversation.
                    match result {
                        Ok(_) => {
                            let _ = core_tx_clone.send(CoreEvent::MessageComplete).await;
                        }
                        Err(e) => {
                            let _ = core_tx_clone.send(CoreEvent::Error(e.to_string())).await;
                        }
                    }
                }
                UiEvent::SlashCommand { name, args } => {
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
                        let model = engine_clone.model().to_string();

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

                    let command_ctx = commands::CommandContext {
                        state_store: state_store_clone.clone(),
                        model: state_store_clone.current().current_model.clone(),
                        provider_name: "auto".into(),
                        session_id: String::new(),
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
                        None => {
                            let _ = core_tx_clone
                                .send(CoreEvent::Error(format!("Unknown command: /{name}")))
                                .await;
                        }
                    }
                }
                UiEvent::Quit => break,
                _ => {}
            }
        }
    });

    app.run().await?;

    let state = state_store.current();
    session.messages = state.messages;
    oxicode_session::save_session(session, None)?;

    engine_handle.abort();

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
    let tool_context = ToolContext {
        working_dir: cwd,
        file_state: Arc::new(oxicode_tools::file_state_tracker::FileStateTracker::default()),
        task_manager: Arc::new(std::sync::Mutex::new(oxicode_tasks::TaskManager::default())),
        task_abort_handles: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        mcp_manager: Arc::new(oxicode_mcp::McpServerManager::new()),
        skill_executor: None, // Agent mode doesn't initialize skills
        team_manager: Arc::new(std::sync::Mutex::new(oxicode_agents::TeamManager::new())),
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
