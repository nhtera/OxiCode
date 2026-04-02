mod commands;
mod completions;
mod structured_output;

use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use oxicode_api::AnthropicProvider;
use oxicode_common::constants;
use oxicode_common::Message;
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

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

    // Validate API key
    let api_key = settings.api_key.as_deref().unwrap_or("").to_string();
    if api_key.is_empty() {
        eprintln!(
            "Error: No API key found. Set {} env var or add api_key to ~/.oxicode/settings.toml",
            constants::ENV_API_KEY
        );
        std::process::exit(1);
    }

    // Load CLAUDE.md / OXICODE.md
    let cwd = std::env::current_dir()?;
    let (global_md, project_md) = oxicode_config::load_claude_md(&cwd);
    let system_prompt = oxicode_core::system_prompt::assemble_system_prompt(
        global_md.as_deref(),
        project_md.as_deref(),
        None, // skills injected at session layer when skill discovery is wired
    );

    let provider = Arc::new(AnthropicProvider::new(api_key));

    let state_store = Arc::new(StateStore::new(AppState {
        current_model: settings.model.clone(),
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
        file_state: std::sync::Arc::new(oxicode_tools::file_state_tracker::FileStateTracker::default()),
        task_manager: std::sync::Arc::new(std::sync::Mutex::new(oxicode_tasks::TaskManager::default())),
        task_abort_handles: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        mcp_manager: mcp_ref.clone(),
    };

    let engine = Arc::new(QueryEngine::new(
        provider,
        state_store.clone(),
        tool_registry,
        permission_pipeline,
        tool_context,
        settings.model.clone(),
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
    }
}

/// Run the interactive TUI.
async fn run_tui(
    engine: Arc<QueryEngine>,
    state_store: Arc<StateStore>,
    session: &mut Session,
    _settings: &Settings,
) -> Result<()> {
    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(32);
    let (core_tx, core_rx) = mpsc::channel::<CoreEvent>(256);

    for msg in &session.messages {
        state_store.push_message(msg.clone());
    }

    let mut app = App::new(&state_store, ui_tx, core_rx);

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
                            let _ =
                                core_tx_clone.send(CoreEvent::Error(e.to_string())).await;
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
