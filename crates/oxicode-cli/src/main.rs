use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use futures::StreamExt;
use oxicode_api::{AnthropicProvider, MessageRequest, StreamEvent};
use oxicode_common::constants;
use oxicode_common::Message;
use oxicode_config::Settings;
use oxicode_core::{Conversation, QueryEngine};
use oxicode_session::Session;
use oxicode_state::{AppState, StateStore};
use oxicode_tui::{App, CoreEvent, UiEvent};
use tokio::sync::mpsc;

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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

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

    let engine = Arc::new(QueryEngine::new(
        provider,
        state_store.clone(),
        settings.model.clone(),
        settings.max_tokens,
        system_prompt,
    ));

    if let Some(prompt) = cli.prompt {
        return run_single_prompt(engine, &mut session, &prompt).await;
    }

    run_tui(engine, state_store, &mut session, &settings).await
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

    match engine.execute_turn(&mut conversation).await {
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

/// Run the interactive TUI.
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

    let engine_clone = engine.clone();
    let core_tx_clone = core_tx.clone();
    let state_store_clone = state_store.clone();
    let model = settings.model.clone();

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
                    state_store_clone.push_message(user_msg.clone());
                    conversation.push(user_msg);

                    let _ = core_tx_clone.send(CoreEvent::StreamStart).await;

                    let request = MessageRequest::new(&model, conversation.api_messages().to_vec())
                        .with_system(engine_clone.system_prompt_ref())
                        .with_max_tokens(engine_clone.max_tokens());

                    match engine_clone.provider_ref().stream_message(request).await {
                        Ok(mut stream) => {
                            let mut full_text = String::new();
                            while let Some(event_result) = stream.next().await {
                                match event_result {
                                    Ok(StreamEvent::TextDelta { text }) => {
                                        full_text.push_str(&text);
                                        let _ =
                                            core_tx_clone.send(CoreEvent::TextDelta(text)).await;
                                    }
                                    Ok(StreamEvent::MessageStop { .. }) => break,
                                    Ok(StreamEvent::Error { message }) => {
                                        let _ = core_tx_clone.send(CoreEvent::Error(message)).await;
                                        break;
                                    }
                                    Ok(_) => {}
                                    Err(e) => {
                                        let _ = core_tx_clone
                                            .send(CoreEvent::Error(e.to_string()))
                                            .await;
                                        break;
                                    }
                                }
                            }

                            let mut assistant_msg = Message::assistant();
                            assistant_msg
                                .content
                                .push(oxicode_common::ContentBlock::Text { text: full_text });
                            state_store_clone.push_message(assistant_msg.clone());
                            conversation.push(assistant_msg);
                            state_store_clone.set_streaming(false);
                        }
                        Err(e) => {
                            let _ = core_tx_clone.send(CoreEvent::Error(e.to_string())).await;
                        }
                    }

                    let _ = core_tx_clone.send(CoreEvent::StreamEnd).await;
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
