//! Live conversation loop integration tests.
//!
//! Tests the full agent loop: user prompt → API → tool use → tool result → response.
//! Requires `ANTHROPIC_AUTH_TOKEN` env var. Gated behind `#[ignore]`.
//! Run with: `cargo test -p oxicode-core --test live_conversation_loop -- --ignored --nocapture`
#![allow(clippy::await_holding_lock)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use oxicode_api::AnthropicProvider;
use oxicode_common::{ContentBlock, Message};
use oxicode_core::{Conversation, QueryEngine};
use oxicode_permissions::pipeline::{PermissionMode, PermissionPipeline};
use oxicode_state::{AppState, StateStore};
use oxicode_tools::file_state_tracker::FileStateTracker;
use oxicode_tools::tool_trait::ToolContext;

/// Serialize live API tests to avoid provider-side 429s from parallel test workers.
static LIVE_API_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Build a ToolContext pointing at the given temp directory.
fn make_tool_context(dir: &Path) -> ToolContext {
    ToolContext {
        working_dir: dir.to_path_buf(),
        extra_working_dirs: Vec::new(),
        file_state: Arc::new(FileStateTracker::default()),
        task_manager: Arc::new(Mutex::new(oxicode_tasks::TaskManager::default())),
        task_abort_handles: Arc::new(Mutex::new(std::collections::HashMap::new())),
        mcp_manager: Arc::new(oxicode_mcp::McpServerManager::new()),
        skill_executor: None,
        team_manager: Arc::new(Mutex::new(oxicode_agents::TeamManager::new())),
        bash_processes: Arc::new(Mutex::new(std::collections::HashMap::new())),
        hook_manager: None,
        permission_mode: oxicode_permissions::PermissionMode::Default,
    }
}

/// Build a live QueryEngine connected to the real API.
fn make_live_engine(dir: &Path) -> (Arc<QueryEngine>, Arc<StateStore>) {
    let token =
        std::env::var("ANTHROPIC_AUTH_TOKEN").expect("ANTHROPIC_AUTH_TOKEN env var required");
    let base_url = std::env::var("ANTHROPIC_BASE_URL")
        .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
    let model = std::env::var("ANTHROPIC_DEFAULT_SONNET_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());

    let provider = Arc::new(AnthropicProvider::new(token).with_base_url(base_url));
    let state_store = Arc::new(StateStore::new(AppState::default()));
    let tool_registry = Arc::new(oxicode_tools::default_registry());
    let permission_pipeline = Arc::new(PermissionPipeline::new(PermissionMode::Bypass, vec![]));
    let tool_context = make_tool_context(dir);

    let engine = Arc::new(QueryEngine::new(
        provider,
        state_store.clone(),
        tool_registry,
        permission_pipeline,
        tool_context,
        model,
        16384,
        "You are a helpful coding assistant. Be concise.".to_string(),
    ));

    (engine, state_store)
}

#[tokio::test]
#[ignore = "live API test — requires ANTHROPIC_AUTH_TOKEN env var"]
async fn test_simple_text_conversation() {
    let _lock = LIVE_API_TEST_LOCK.lock().expect("live test lock");
    let tmp = tempfile::tempdir().unwrap();
    let (engine, state_store) = make_live_engine(tmp.path());

    let mut conversation = Conversation::new();
    conversation.push(Message::user(
        "Say exactly the word 'pineapple' and nothing else.",
    ));

    let result = engine.execute_turn(&mut conversation, None).await;
    let assistant_msg = result.expect("execute_turn should succeed");

    assert_eq!(
        assistant_msg.role,
        oxicode_common::Role::Assistant,
        "Response should be from assistant"
    );

    let text = assistant_msg.text().to_lowercase();
    assert!(
        text.contains("pineapple"),
        "Response should contain 'pineapple', got: {text}"
    );

    // Verify state store tracked the assistant message.
    let state = state_store.current();
    assert!(
        !state.messages.is_empty(),
        "State should have at least 1 message (assistant), got: {}",
        state.messages.len()
    );
}

#[tokio::test]
#[ignore = "live API test — requires ANTHROPIC_AUTH_TOKEN env var"]
async fn test_conversation_with_tool_use() {
    let _lock = LIVE_API_TEST_LOCK.lock().expect("live test lock");
    let tmp = tempfile::tempdir().unwrap();

    // Create a file for the model to read.
    let test_file = tmp.path().join("secret.txt");
    std::fs::write(&test_file, "The secret word is: OXICODE42").unwrap();

    let (engine, _state_store) = make_live_engine(tmp.path());

    let mut conversation = Conversation::new();
    let prompt = format!(
        "Read the file at {} and tell me the secret word. Use the file_read tool.",
        test_file.display()
    );
    conversation.push(Message::user(&prompt));

    let result = engine.execute_turn(&mut conversation, None).await;
    let assistant_msg = result.expect("execute_turn with tool use should succeed");

    // The conversation should now contain the full loop:
    // 1. User message
    // 2. Assistant with ToolUse
    // 3. User with ToolResult
    // 4. Assistant with final text
    assert!(
        conversation.len() >= 3,
        "Conversation should have at least 3 messages after tool use, got: {}",
        conversation.len()
    );

    // The final assistant message should contain the secret word.
    let final_text = assistant_msg.text();
    assert!(
        final_text.contains("OXICODE42"),
        "Final response should contain the secret word 'OXICODE42', got: {final_text}"
    );
}

#[tokio::test]
#[ignore = "live API test — requires ANTHROPIC_AUTH_TOKEN env var"]
async fn test_conversation_bash_tool() {
    let _lock = LIVE_API_TEST_LOCK.lock().expect("live test lock");
    let tmp = tempfile::tempdir().unwrap();
    let (engine, _) = make_live_engine(tmp.path());

    let mut conversation = Conversation::new();
    conversation.push(Message::user(
        "Run the bash command `echo RUSTTEST123` and tell me the output. Use the bash tool.",
    ));

    let result = engine.execute_turn(&mut conversation, None).await;
    let assistant_msg = result.expect("bash tool conversation should succeed");

    let final_text = assistant_msg.text();
    assert!(
        final_text.contains("RUSTTEST123"),
        "Response should contain bash output 'RUSTTEST123', got: {final_text}"
    );
}

#[tokio::test]
#[ignore = "live API test — requires ANTHROPIC_AUTH_TOKEN env var"]
async fn test_conversation_glob_tool() {
    let _lock = LIVE_API_TEST_LOCK.lock().expect("live test lock");
    let tmp = tempfile::tempdir().unwrap();

    // Create some files for glob to find.
    std::fs::write(tmp.path().join("alpha.rs"), "fn alpha() {}").unwrap();
    std::fs::write(tmp.path().join("beta.rs"), "fn beta() {}").unwrap();
    std::fs::write(tmp.path().join("gamma.txt"), "not rust").unwrap();

    let (engine, _) = make_live_engine(tmp.path());

    let mut conversation = Conversation::new();
    let prompt = format!(
        "Use the glob tool to find all .rs files in {}. List the filenames you find.",
        tmp.path().display()
    );
    conversation.push(Message::user(&prompt));

    let result = engine.execute_turn(&mut conversation, None).await;
    let assistant_msg = result.expect("glob tool conversation should succeed");

    let final_text = assistant_msg.text();
    assert!(
        final_text.contains("alpha.rs") || final_text.contains("beta.rs"),
        "Response should mention .rs files, got: {final_text}"
    );
}

#[tokio::test]
#[ignore = "live API test — requires ANTHROPIC_AUTH_TOKEN env var"]
async fn test_conversation_multi_tool_sequence() {
    let _lock = LIVE_API_TEST_LOCK.lock().expect("live test lock");
    let tmp = tempfile::tempdir().unwrap();

    // Create a file.
    let file_path = tmp.path().join("data.txt");
    std::fs::write(&file_path, "initial content").unwrap();

    let (engine, _) = make_live_engine(tmp.path());

    let mut conversation = Conversation::new();
    let prompt = format!(
        "Do these steps in order using tools:\n\
         1. Read the file at {}\n\
         2. Write a new file at {} with content 'modified'\n\
         3. Read the new file to confirm\n\
         Tell me the content of each read.",
        file_path.display(),
        tmp.path().join("output.txt").display()
    );
    conversation.push(Message::user(&prompt));

    let result = engine.execute_turn(&mut conversation, None).await;
    let assistant_msg = result.expect("multi-tool conversation should succeed");

    // Verify the output file was created.
    let output_path = tmp.path().join("output.txt");
    assert!(
        output_path.exists(),
        "output.txt should have been created by file_write tool"
    );

    let final_text = assistant_msg.text();
    assert!(
        final_text.contains("initial content") || final_text.contains("modified"),
        "Response should mention file contents, got: {final_text}"
    );
}

#[tokio::test]
#[ignore = "live API test — requires ANTHROPIC_AUTH_TOKEN env var"]
async fn test_conversation_state_has_usage() {
    let _lock = LIVE_API_TEST_LOCK.lock().expect("live test lock");
    let tmp = tempfile::tempdir().unwrap();
    let (engine, state_store) = make_live_engine(tmp.path());

    let mut conversation = Conversation::new();
    conversation.push(Message::user("Say hi"));

    let _ = engine.execute_turn(&mut conversation, None).await;

    let state = state_store.current();
    assert!(
        state.total_usage.input_tokens > 0 || state.total_usage.output_tokens > 0,
        "State should track token usage after a turn. Input: {}, Output: {}",
        state.total_usage.input_tokens,
        state.total_usage.output_tokens
    );
}

// ── Non-API tests (no #[ignore]) ───────────────────────────────────

#[test]
fn test_conversation_push_and_api_messages() {
    let mut conv = Conversation::new();
    assert!(conv.is_empty());

    conv.push(Message::user("hello"));
    assert_eq!(conv.len(), 1);

    let api = conv.api_messages();
    assert_eq!(api.len(), 1);
    assert_eq!(api[0].text(), "hello");
}

#[test]
fn test_conversation_with_tool_blocks() {
    let mut conv = Conversation::new();
    conv.push(Message::user("read a file"));

    let mut assistant = Message::assistant();
    assistant.content.push(ContentBlock::ToolUse {
        id: "tu_1".to_string(),
        name: "file_read".to_string(),
        input: serde_json::json!({"file_path": "/tmp/test.txt"}),
    });
    conv.push(assistant);

    let mut tool_result = Message::user("");
    tool_result.content = vec![ContentBlock::ToolResult {
        tool_use_id: "tu_1".to_string(),
        content: "file contents here".to_string(),
        is_error: false,
    }];
    conv.push(tool_result);

    assert_eq!(conv.len(), 3);

    let api = conv.api_messages();
    assert_eq!(api.len(), 3);
}
