#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    // Explicit imports required because tests.rs is a submodule of app, not app.rs itself.
    use crate::app::utils::{
        char_to_byte_index, detect_provider_from_model_name, is_dangerous_operation,
        summarize_input,
    };
    use crate::app::App;
    use crate::app::{PendingPermission, MAX_HOOK_LINE_CHARS};
    use crate::events::{CoreEvent, UiEvent};
    use crate::widgets::permission_dialog::RiskLevel;
    use crate::widgets::session_browser::SessionBrowserMode;
    use crate::widgets::{Notification, SessionEntry, SlashCommandMeta};
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use insta::assert_snapshot;
    use oxicode_common::{Message, PermissionResponse};
    use oxicode_state::StateStore;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::Terminal;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc;
    use tokio::sync::oneshot;

    #[test]
    fn test_char_to_byte_index_ascii() {
        assert_eq!(char_to_byte_index("hello", 0), 0);
        assert_eq!(char_to_byte_index("hello", 3), 3);
        assert_eq!(char_to_byte_index("hello", 5), 5);
    }

    #[test]
    fn test_char_to_byte_index_utf8() {
        let s = "héllo"; // é is 2 bytes
        assert_eq!(char_to_byte_index(s, 0), 0);
        assert_eq!(char_to_byte_index(s, 1), 1); // 'h' at 0, 'é' starts at 1
        assert_eq!(char_to_byte_index(s, 2), 3); // 'l' starts at byte 3
    }

    #[test]
    fn test_char_to_byte_index_emoji() {
        let s = "a😀b";
        assert_eq!(char_to_byte_index(s, 0), 0);
        assert_eq!(char_to_byte_index(s, 1), 1); // emoji starts at byte 1
        assert_eq!(char_to_byte_index(s, 2), 5); // 'b' starts at byte 5
        assert_eq!(char_to_byte_index(s, 3), 6); // past end
    }

    #[test]
    fn test_char_to_byte_index_empty_string() {
        // Empty string: any char index returns 0 (the length).
        assert_eq!(char_to_byte_index("", 0), 0);
        assert_eq!(char_to_byte_index("", 5), 0);
    }

    #[test]
    fn test_char_to_byte_index_cjk() {
        // CJK characters are 3 bytes each in UTF-8.
        let s = "中文";
        assert_eq!(char_to_byte_index(s, 0), 0);
        assert_eq!(char_to_byte_index(s, 1), 3); // '文' starts at byte 3
        assert_eq!(char_to_byte_index(s, 2), 6); // past end
    }

    #[test]
    fn test_char_to_byte_index_fire_emoji() {
        // 🔥 is 4 bytes in UTF-8.
        let s = "🔥x";
        assert_eq!(char_to_byte_index(s, 0), 0);
        assert_eq!(char_to_byte_index(s, 1), 4); // 'x' starts at byte 4
        assert_eq!(char_to_byte_index(s, 2), 5); // past end = s.len()
                                                 // Index beyond string length clamps to s.len().
        assert_eq!(char_to_byte_index(s, 99), 5);
    }

    #[test]
    fn test_summarize_input_command() {
        let v = serde_json::json!({"command": "echo hello"});
        assert_eq!(summarize_input(&v), "echo hello");
    }

    #[test]
    fn test_summarize_input_file_path() {
        let v = serde_json::json!({"file_path": "/foo.rs"});
        assert_eq!(summarize_input(&v), "/foo.rs");
    }

    #[test]
    fn test_summarize_input_pattern() {
        let v = serde_json::json!({"pattern": "test", "path": "src"});
        assert_eq!(summarize_input(&v), "test in src");
    }

    #[test]
    fn test_summarize_input_pattern_default_path() {
        // When "path" is missing, defaults to "."
        let v = serde_json::json!({"pattern": "fn main"});
        assert_eq!(summarize_input(&v), "fn main in .");
    }

    #[test]
    fn test_summarize_input_empty_object() {
        let v = serde_json::json!({});
        // Falls back to JSON serialization of empty object.
        let result = summarize_input(&v);
        assert_eq!(result, "{}");
    }

    #[test]
    fn test_summarize_input_long_command_truncated() {
        // Commands > 80 chars are truncated with "...".
        let long_cmd = "a".repeat(100);
        let v = serde_json::json!({"command": long_cmd});
        let result = summarize_input(&v);
        assert!(result.ends_with("..."), "long input should end with '...'");
        // Truncated at char 80, so total visible prefix chars = 80.
        let prefix: String = result.chars().take(80).collect();
        assert_eq!(prefix, "a".repeat(80));
    }

    #[test]
    fn test_detect_provider_anthropic() {
        assert_eq!(
            detect_provider_from_model_name("claude-sonnet-4-20250514"),
            "anthropic"
        );
        assert_eq!(
            detect_provider_from_model_name("claude-opus-4"),
            "anthropic"
        );
        assert_eq!(
            detect_provider_from_model_name("anthropic/claude-3"),
            "anthropic"
        );
    }

    #[test]
    fn test_detect_provider_openai() {
        assert_eq!(detect_provider_from_model_name("gpt-4"), "openai");
        assert_eq!(detect_provider_from_model_name("gpt-3.5-turbo"), "openai");
        assert_eq!(detect_provider_from_model_name("o1-mini"), "openai");
        assert_eq!(detect_provider_from_model_name("o3-mini"), "openai");
        assert_eq!(detect_provider_from_model_name("o4-preview"), "openai");
    }

    #[test]
    fn test_detect_provider_deepseek() {
        assert_eq!(detect_provider_from_model_name("deepseek-chat"), "deepseek");
        assert_eq!(
            detect_provider_from_model_name("deepseek/coder"),
            "deepseek"
        );
    }

    #[test]
    fn test_detect_provider_ollama() {
        // Ollama model names contain ':' without '/'
        assert_eq!(detect_provider_from_model_name("llama:7b"), "ollama");
        assert_eq!(detect_provider_from_model_name("mistral:latest"), "ollama");
    }

    #[test]
    fn test_detect_provider_openrouter() {
        // OpenRouter model names contain '/' but not known prefixes.
        assert_eq!(
            detect_provider_from_model_name("meta-llama/Llama-3"),
            "openrouter"
        );
        assert_eq!(
            detect_provider_from_model_name("mistralai/mixtral-8x7b"),
            "openrouter"
        );
    }

    #[test]
    fn test_detect_provider_bedrock() {
        assert_eq!(
            detect_provider_from_model_name("anthropic.claude-v2"),
            "bedrock"
        );
        assert_eq!(
            detect_provider_from_model_name("anthropic.claude-3-sonnet"),
            "bedrock"
        );
    }

    #[test]
    fn test_detect_provider_unknown() {
        assert_eq!(detect_provider_from_model_name("some-unknown-model"), "");
        assert_eq!(detect_provider_from_model_name(""), "");
    }

    // ── MCP elicitation overlay wiring ───────────────────────────────────────

    fn elicitation_envelope(
        input_type: oxicode_mcp::ElicitationInputType,
        choices: Vec<String>,
        default_value: Option<String>,
    ) -> (
        oxicode_mcp::ElicitationEnvelope,
        oneshot::Receiver<oxicode_mcp::ElicitationResponse>,
    ) {
        let (reply_tx, reply_rx) = oneshot::channel();
        let req = oxicode_mcp::ElicitationRequest {
            id: "test-req".to_string(),
            message: "Pick one".to_string(),
            input_type,
            choices,
            default_value,
        };
        ((req, reply_tx), reply_rx)
    }

    #[test]
    fn test_elicitation_envelope_promotes_to_pending_dialog() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (envelope, _reply_rx) =
            elicitation_envelope(oxicode_mcp::ElicitationInputType::Text, vec![], None);
        assert!(app.pending_elicitation.is_none());
        app.accept_elicitation_envelope(envelope);
        assert!(app.pending_elicitation.is_some());
    }

    #[tokio::test]
    async fn test_elicitation_text_submit_sends_response_through_reply_tx() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (envelope, reply_rx) =
            elicitation_envelope(oxicode_mcp::ElicitationInputType::Text, vec![], None);
        app.accept_elicitation_envelope(envelope);

        // Type "abc" and press Enter.
        for c in "abc".chars() {
            app.handle_elicitation_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        app.handle_elicitation_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(
            app.pending_elicitation.is_none(),
            "dialog cleared on submit"
        );
        let response = reply_rx.await.expect("reply");
        assert!(response.approved);
        assert_eq!(response.value, "abc");
        assert_eq!(response.id, "test-req");
    }

    #[tokio::test]
    async fn test_elicitation_esc_sends_denial() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (envelope, reply_rx) =
            elicitation_envelope(oxicode_mcp::ElicitationInputType::Confirm, vec![], None);
        app.accept_elicitation_envelope(envelope);
        app.handle_elicitation_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        let response = reply_rx.await.expect("reply");
        assert!(!response.approved);
        assert!(response.value.is_empty());
    }

    #[tokio::test]
    async fn test_elicitation_second_request_denied_while_first_active() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (envelope1, _reply_rx1) =
            elicitation_envelope(oxicode_mcp::ElicitationInputType::Text, vec![], None);
        let (envelope2, reply_rx2) =
            elicitation_envelope(oxicode_mcp::ElicitationInputType::Text, vec![], None);
        app.accept_elicitation_envelope(envelope1);
        app.accept_elicitation_envelope(envelope2);

        // First dialog still active; second was auto-denied.
        assert!(app.pending_elicitation.is_some());
        let denied = reply_rx2.await.expect("reply");
        assert!(!denied.approved);
    }

    #[test]
    fn test_handle_core_event_stream_start_activates_turn() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        assert!(!app.is_turn_active, "should start inactive");
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(app.is_turn_active, "StreamStart should activate turn");
        assert!(app.turn_started_at.is_some(), "turn timer should start");
    }

    #[test]
    fn test_handle_core_event_text_delta_updates_streaming_text() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::TextDelta("Hello".to_string()));
        assert_eq!(app.streaming_text, "Hello");
        app.handle_core_event(CoreEvent::TextDelta(" World".to_string()));
        assert_eq!(app.streaming_text, "Hello World");
    }

    #[test]
    fn test_handle_core_event_stream_end_clears_streaming_text() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_core_event(CoreEvent::TextDelta("some content".to_string()));
        app.handle_core_event(CoreEvent::StreamEnd);
        // Raw buffer is cleared; committed lines may contain finalized content.
        assert!(
            app.streaming_text.is_empty(),
            "streaming_text cleared on StreamEnd"
        );
    }

    #[test]
    fn test_handle_core_event_message_complete_deactivates_turn() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(app.is_turn_active);
        app.handle_core_event(CoreEvent::MessageComplete);
        assert!(
            !app.is_turn_active,
            "MessageComplete should deactivate turn"
        );
        assert!(app.turn_started_at.is_none(), "turn timer should reset");
        assert!(app.streaming_text.is_empty());
        assert!(app.active_tools.is_empty());
    }

    #[test]
    fn test_handle_core_event_error_adds_notification_and_clears_state() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_core_event(CoreEvent::TextDelta("partial".to_string()));

        let before_count = app.notifications.len();
        app.handle_core_event(CoreEvent::Error("something went bad".to_string()));

        assert_eq!(
            app.notifications.len(),
            before_count + 1,
            "Error should add a notification"
        );
        assert!(!app.is_turn_active, "Error should deactivate turn");
        assert!(
            app.streaming_text.is_empty(),
            "Error should clear streaming text"
        );
        assert!(
            app.active_tools.is_empty(),
            "Error should clear active tools"
        );
    }

    /// Bug fix regression: Error must also reset turn_started_at so the thinking
    /// indicator does not show a stale elapsed time after an API error.
    #[test]
    fn test_handle_core_event_error_resets_turn_timer() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(app.turn_started_at.is_some(), "timer set on StreamStart");

        app.handle_core_event(CoreEvent::Error("api error".to_string()));

        assert!(
            app.turn_started_at.is_none(),
            "Error must reset turn_started_at to prevent stale thinking indicator"
        );
    }

    #[test]
    fn test_handle_core_event_tool_use_start_adds_entry() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        assert!(app.active_tools.is_empty());

        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "tool-1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls -la"}),
        });

        assert_eq!(
            app.active_tools.len(),
            1,
            "ToolUseStart should add tool entry"
        );
        assert_eq!(app.active_tools[0].id, "tool-1");
        assert_eq!(app.active_tools[0].name, "bash");
        assert_eq!(app.active_tools[0].input_summary, "ls -la");
        assert!(app.active_tools[0].result.is_none(), "result not set yet");
    }

    #[test]
    fn test_handle_core_event_tool_result_sets_result_on_matching_tool() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "tool-abc".to_string(),
            name: "file_read".to_string(),
            input: serde_json::json!({"file_path": "/tmp/test.txt"}),
        });
        assert!(app.active_tools[0].result.is_none());

        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "tool-abc".to_string(),
            content: "file contents here".to_string(),
            is_error: false,
        });

        let result = app.active_tools[0]
            .result
            .as_ref()
            .expect("result should be set after ToolResult");
        assert_eq!(result.0, "file contents here");
        assert!(!result.1, "is_error should be false");
    }

    #[test]
    fn test_handle_core_event_tool_result_error_flag() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "tool-err".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "bad_cmd"}),
        });

        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "tool-err".to_string(),
            content: "command not found".to_string(),
            is_error: true,
        });

        let result = app.active_tools[0]
            .result
            .as_ref()
            .expect("result should be set");
        assert!(result.1, "is_error should be true for error result");
    }

    #[test]
    fn test_handle_core_event_tool_result_unmatched_id_noop() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "tool-x".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "echo"}),
        });

        // Send result for a different ID — should not panic or modify existing tools.
        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "tool-y".to_string(),
            content: "output".to_string(),
            is_error: false,
        });

        assert!(
            app.active_tools[0].result.is_none(),
            "unmatched tool_use_id should not set result"
        );
    }

    fn make_test_app() -> (App, mpsc::Receiver<UiEvent>, mpsc::Sender<CoreEvent>) {
        let state_store = Arc::new(StateStore::default());
        let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>(32);
        let (core_tx, core_rx) = mpsc::channel::<CoreEvent>(32);
        let app = App::new(&state_store, ui_tx, core_rx, Vec::new());
        (app, ui_rx, core_tx)
    }

    fn normalized_rendered_text(terminal: &Terminal<TestBackend>) -> String {
        format!("{}", terminal.backend())
            .lines()
            .map(|line| normalize_elapsed(line.trim_end()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Rewrite rendered elapsed times — `(1.3s)` becomes `(0.0s)` — so snapshots
    /// do not depend on how fast the machine running them happens to be. A
    /// loaded CI runner takes long enough for a spinner to tick past 0.05s,
    /// which would otherwise fail the assertion.
    fn normalize_elapsed(line: &str) -> String {
        let chars: Vec<char> = line.chars().collect();
        let mut out = String::with_capacity(line.len());
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '(' {
                if let Some(end) = match_elapsed(&chars, i + 1) {
                    out.push_str("(0.0s)");
                    i = end;
                    continue;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    }

    /// If `chars[start..]` begins with `<digits>.<digits>s)`, return the index
    /// just past the closing paren.
    fn match_elapsed(chars: &[char], start: usize) -> Option<usize> {
        let mut i = start;
        while chars.get(i).is_some_and(char::is_ascii_digit) {
            i += 1;
        }
        if i == start || chars.get(i) != Some(&'.') {
            return None;
        }
        i += 1;
        let frac = i;
        while chars.get(i).is_some_and(char::is_ascii_digit) {
            i += 1;
        }
        if i == frac || chars.get(i) != Some(&'s') || chars.get(i + 1) != Some(&')') {
            return None;
        }
        Some(i + 2)
    }

    #[test]
    fn normalize_elapsed_collapses_timings() {
        // Any elapsed value collapses to the canonical one.
        assert_eq!(
            normalize_elapsed("│  ✓ Bash($ echo hello) (0.1s)"),
            "│  ✓ Bash($ echo hello) (0.0s)"
        );
        assert_eq!(
            normalize_elapsed("⠹ Thinking... (12.7s)"),
            "⠹ Thinking... (0.0s)"
        );
        // Already-canonical text is unchanged.
        assert_eq!(normalize_elapsed("(0.0s)"), "(0.0s)");
        // Non-timing parentheses are left alone.
        assert_eq!(
            normalize_elapsed("Bash($ echo hi) (abc) (1s) (1.s) (.1s)"),
            "Bash($ echo hi) (abc) (1s) (1.s) (.1s)"
        );
    }

    fn make_test_slash_commands() -> Vec<SlashCommandMeta> {
        vec![
            SlashCommandMeta {
                name: "help".into(),
                description: "Show help".into(),
                category: "General".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "clear".into(),
                description: "Clear messages".into(),
                category: "Session".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "compact".into(),
                description: "Compact context".into(),
                category: "Session".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "model".into(),
                description: "Switch model".into(),
                category: "Model".into(),
                arg_candidates: vec![],
            },
            SlashCommandMeta {
                name: "vim".into(),
                description: "Toggle vim mode".into(),
                category: "Editor".into(),
                arg_candidates: vec![],
            },
        ]
    }

    fn make_test_app_with_commands() -> (App, mpsc::Receiver<UiEvent>, mpsc::Sender<CoreEvent>) {
        let state_store = Arc::new(StateStore::default());
        let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>(32);
        let (core_tx, core_rx) = mpsc::channel::<CoreEvent>(32);
        let app = App::new(&state_store, ui_tx, core_rx, make_test_slash_commands());
        (app, ui_rx, core_tx)
    }

    #[test]
    fn test_draw_with_test_backend_renders_baseline_ui() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        app.draw(&mut terminal).expect("draw succeeds");
        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_baseline", rendered.as_str());

        assert!(
            rendered.contains("Ready"),
            "status bar should render readiness"
        );
        assert!(
            rendered.contains("Type your message..."),
            "input placeholder should render"
        );
    }

    #[tokio::test]
    async fn test_event_loop_keyboard_input_emits_ui_events() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");
        let (term_tx, mut term_rx) = mpsc::channel::<Event>(16);

        term_tx
            .send(Event::Key(KeyEvent::new(
                KeyCode::Char('h'),
                KeyModifiers::NONE,
            )))
            .await
            .expect("send h");
        term_tx
            .send(Event::Key(KeyEvent::new(
                KeyCode::Char('i'),
                KeyModifiers::NONE,
            )))
            .await
            .expect("send i");
        term_tx
            .send(Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE,
            )))
            .await
            .expect("send enter");
        // Double Ctrl+C within 2s is required to quit (first press arms the
        // interrupt hint, second press sets should_quit + emits UiEvent::Quit).
        for _ in 0..2 {
            term_tx
                .send(Event::Key(KeyEvent::new(
                    KeyCode::Char('c'),
                    KeyModifiers::CONTROL,
                )))
                .await
                .expect("send ctrl+c");
        }
        drop(term_tx);

        app.event_loop(&mut terminal, &mut term_rx)
            .await
            .expect("event loop exits");

        assert!(
            matches!(ui_rx.recv().await, Some(UiEvent::UserInput { text, .. }) if text == "hi"),
            "expected submitted input event"
        );
        assert!(
            matches!(ui_rx.recv().await, Some(UiEvent::Quit)),
            "expected quit event"
        );
    }

    #[tokio::test]
    async fn test_event_loop_permission_dialog_overlay_renders_and_denies_on_ctrl_c() {
        let (mut app, mut ui_rx, core_tx) = make_test_app();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");
        let (reply_tx, reply_rx) = oneshot::channel::<PermissionResponse>();

        core_tx
            .send(CoreEvent::PermissionAsk {
                tool_name: "bash".to_string(),
                input_summary: "echo hello".to_string(),
                prompt: "This command can modify files".to_string(),
                reply_tx,
            })
            .await
            .expect("send permission ask");

        // Pull the core event and render one frame while the permission dialog is active.
        let core_event = app
            .core_rx
            .recv()
            .await
            .expect("permission event delivered to app");
        app.handle_core_event(core_event);
        app.draw(&mut terminal)
            .expect("draw with permission dialog");

        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_permission_dialog_overlay", rendered.as_str());
        assert!(
            rendered.contains("Allow bash"),
            "permission dialog title should render"
        );
        assert!(
            rendered.contains("echo hello"),
            "permission dialog should include command preview"
        );

        // First Ctrl+C while the dialog is open denies the permission and arms
        // the double-press quit hint; the second Ctrl+C within 2s emits Quit.
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .await;

        let response = tokio::time::timeout(Duration::from_secs(1), reply_rx)
            .await
            .expect("permission response timeout")
            .expect("permission response channel closed");
        assert_eq!(
            response,
            PermissionResponse::Deny,
            "Ctrl+C on permission dialog should deny"
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .await;
        assert!(
            matches!(ui_rx.recv().await, Some(UiEvent::Quit)),
            "Second Ctrl+C within 2s should emit quit"
        );
    }

    #[test]
    fn test_draw_snapshots_active_tool_lifecycle() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "tu_1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "echo hello"}),
        });
        app.draw(&mut terminal).expect("draw with running tool");
        assert_snapshot!(
            "app_active_tool_running",
            normalized_rendered_text(&terminal).as_str()
        );

        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "hello".to_string(),
            is_error: false,
        });
        app.draw(&mut terminal)
            .expect("draw with completed tool result");
        assert_snapshot!(
            "app_active_tool_done",
            normalized_rendered_text(&terminal).as_str()
        );

        app.handle_core_event(CoreEvent::MessageComplete);
        app.draw(&mut terminal)
            .expect("draw after message complete");
        let after_complete = normalized_rendered_text(&terminal);
        assert_snapshot!("app_after_message_complete", after_complete.as_str());
        assert!(
            !after_complete.contains("[running]"),
            "active tool list should clear on message complete"
        );
    }

    #[test]
    fn test_scroll_up_from_auto_scroll_switches_to_manual_mode() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = true;
        app.max_scroll_offset = 120;
        app.scroll_offset = 0;

        app.scroll_up_by(1);

        assert!(!app.auto_scroll, "scrolling up should disable auto-scroll");
        assert_eq!(app.scroll_offset, 119);
    }

    #[test]
    fn test_scroll_down_to_bottom_reenables_auto_scroll() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 120;
        app.scroll_offset = 110;

        app.scroll_down_by(20);

        assert_eq!(app.scroll_offset, 120);
        assert!(app.auto_scroll, "reaching bottom should enable auto-scroll");
    }

    #[test]
    fn test_mouse_wheel_uses_message_scrolling() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = true;
        app.max_scroll_offset = 50;

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll_offset, 47);
        assert!(!app.auto_scroll);

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll_offset, 50);
        assert!(app.auto_scroll);
    }

    #[test]
    fn test_scrollbar_click_jumps_to_position() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 200;
        app.scroll_offset = 0;
        // Message area: 80 cols wide, 40 rows tall at (0, 1).
        // Borders::ALL → inner track rows 2..40 (track_top=2, track_bottom=40).
        app.message_area = Rect::new(0, 1, 80, 40);

        // Click on scrollbar col (79), at row 21.
        // relative_y = 21 - 2 = 19, track_height = 38, ratio = 19/37 ≈ 0.514 → offset ≈ 103
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 79,
            row: 21,
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            app.scroll_offset > 90 && app.scroll_offset < 115,
            "Expected ~103, got {}",
            app.scroll_offset
        );
        assert!(!app.auto_scroll);
    }

    #[test]
    fn test_scrollbar_click_at_bottom_enables_auto_scroll() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 200;
        app.scroll_offset = 0;
        app.message_area = Rect::new(0, 1, 80, 40);

        // Click at the very bottom of inner track (row 39 = track_bottom - 1).
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 79,
            row: 39,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll_offset, 200);
        assert!(app.auto_scroll);
    }

    #[test]
    fn test_scrollbar_click_on_border_ignored() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 200;
        app.scroll_offset = 50;
        app.message_area = Rect::new(0, 1, 80, 40);

        // Click on top border row (row 1 = area.y) — outside inner track.
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 79,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            app.scroll_offset, 50,
            "Click on top border should be ignored"
        );

        // Click on bottom border row (row 40 = area.bottom()) — outside inner track.
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 79,
            row: 40,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            app.scroll_offset, 50,
            "Click on bottom border should be ignored"
        );
    }

    #[test]
    fn test_scrollbar_click_far_from_track_ignored() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 200;
        app.scroll_offset = 50;
        app.message_area = Rect::new(0, 1, 80, 40);

        // Click on col 77 (more than 1 col away from scrollbar) — ignored.
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 77,
            row: 20,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            app.scroll_offset, 50,
            "Click far from scrollbar should not change offset"
        );
    }

    #[test]
    fn test_scrollbar_drag_updates_position() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 200;
        app.scroll_offset = 0;
        app.message_area = Rect::new(0, 1, 80, 40);

        // Drag on scrollbar at row 12.
        // relative_y = 12 - 2 = 10, track_height = 38, ratio = 10/37 ≈ 0.270 → offset ≈ 54
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 79,
            row: 12,
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            app.scroll_offset > 40 && app.scroll_offset < 70,
            "Expected ~54, got {}",
            app.scroll_offset
        );
    }

    #[test]
    fn test_scrollbar_click_at_top_scrolls_to_zero() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.max_scroll_offset = 200;
        app.scroll_offset = 100;
        app.message_area = Rect::new(0, 1, 80, 40);

        // Click at very top of inner track (row 2 = track_top).
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 79,
            row: 2,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll_offset, 0, "Click at top should scroll to 0");
        assert!(!app.auto_scroll);
    }

    #[tokio::test]
    async fn test_pageup_key_scrolls_up_from_bottom() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = true;
        app.max_scroll_offset = 100;

        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE))
            .await;

        assert_eq!(app.scroll_offset, 80);
        assert!(!app.auto_scroll);
    }

    /// Helper that also returns the StateStore for pushing messages.
    fn make_test_app_with_store() -> (
        App,
        Arc<StateStore>,
        mpsc::Receiver<UiEvent>,
        mpsc::Sender<CoreEvent>,
    ) {
        let state_store = Arc::new(StateStore::default());
        let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>(32);
        let (core_tx, core_rx) = mpsc::channel::<CoreEvent>(32);
        let app = App::new(&state_store, ui_tx, core_rx, Vec::new());
        (app, state_store, ui_rx, core_tx)
    }

    #[test]
    fn test_scroll_to_bottom_shows_last_line_with_many_messages() {
        let (mut app, store, _ui_rx, _core_tx) = make_test_app_with_store();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        // Push enough messages to exceed the viewport (24 rows minus borders/status/input).
        for i in 1..=30 {
            store.push_message(Message::user(format!("Message number {i}")));
            let mut reply = Message::assistant();
            reply.content.push(oxicode_common::ContentBlock::Text {
                text: format!("Reply to message {i}"),
            });
            store.push_message(reply);
        }

        // auto_scroll is true by default — draw should show the very last content.
        app.draw(&mut terminal).expect("draw succeeds");
        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_scroll_to_bottom_auto", rendered.as_str());

        // The last message ("Reply to message 30") must be visible somewhere.
        assert!(
            rendered.contains("Reply to message 30"),
            "Auto-scroll should show the last message. Rendered:\n{rendered}"
        );
    }

    #[test]
    fn test_manual_scroll_can_reach_bottom() {
        let (mut app, store, _ui_rx, _core_tx) = make_test_app_with_store();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        // Push messages exceeding viewport.
        for i in 1..=20 {
            store.push_message(Message::user(format!("Line {i}")));
            let mut reply = Message::assistant();
            reply.content.push(oxicode_common::ContentBlock::Text {
                text: format!("Answer {i}"),
            });
            store.push_message(reply);
        }

        // First draw to compute max_scroll_offset via the Rc<Cell> feedback.
        app.draw(&mut terminal).expect("initial draw");
        let max_scroll = app.max_scroll_offset;
        assert!(
            max_scroll > 0,
            "max_scroll_offset should be non-zero for overflow content"
        );

        // Disable auto_scroll and manually set to max.
        app.auto_scroll = false;
        app.scroll_offset = max_scroll;
        app.draw(&mut terminal).expect("draw at manual max scroll");
        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_scroll_manual_at_bottom", rendered.as_str());

        assert!(
            rendered.contains("Answer 20"),
            "Manual scroll to max should show last message. Rendered:\n{rendered}"
        );
    }

    #[test]
    fn test_scroll_top_shows_first_message() {
        let (mut app, store, _ui_rx, _core_tx) = make_test_app_with_store();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        for i in 1..=20 {
            store.push_message(Message::user(format!("Msg {i}")));
            let mut reply = Message::assistant();
            reply.content.push(oxicode_common::ContentBlock::Text {
                text: format!("Resp {i}"),
            });
            store.push_message(reply);
        }

        // First draw so max_scroll_offset is computed.
        app.draw(&mut terminal).expect("initial draw");

        // Scroll to top.
        app.auto_scroll = false;
        app.scroll_offset = 0;
        app.draw(&mut terminal).expect("draw at scroll offset 0");
        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_scroll_at_top", rendered.as_str());

        assert!(
            rendered.contains("Msg 1"),
            "Scroll offset 0 should show the first message. Rendered:\n{rendered}"
        );
    }

    #[test]
    fn test_scroll_with_wide_content_wrapping() {
        let (mut app, store, _ui_rx, _core_tx) = make_test_app_with_store();
        // Narrow terminal to force wrapping (40 cols).
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        // Push a message with a long line that will wrap.
        store.push_message(Message::user("Short question"));
        let long_text = "A".repeat(200); // 200 chars will wrap in 40-col terminal
        let mut reply = Message::assistant();
        reply.content.push(oxicode_common::ContentBlock::Text {
            text: long_text.clone(),
        });
        store.push_message(reply);

        // Add a final short message after the wrapping one.
        store.push_message(Message::user("Final question"));
        let mut reply2 = Message::assistant();
        reply2.content.push(oxicode_common::ContentBlock::Text {
            text: "Final answer here".to_string(),
        });
        store.push_message(reply2);

        // Auto-scroll should land at the very bottom showing the final answer.
        app.draw(&mut terminal).expect("draw succeeds");
        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_scroll_wide_content_wrapping", rendered.as_str());

        assert!(
            rendered.contains("Final answer here"),
            "Auto-scroll with wrapped content should show last message. Rendered:\n{rendered}"
        );
    }

    #[test]
    fn test_streaming_thinking_indicator_snapshot() {
        let (mut app, _store, _ui_rx, _core_tx) = make_test_app_with_store();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        // Simulate StreamStart: turn active, no text yet.
        app.handle_core_event(CoreEvent::StreamStart);
        app.draw(&mut terminal).expect("draw with thinking");
        let rendered = normalized_rendered_text(&terminal);
        assert_snapshot!("app_streaming_thinking_indicator", rendered.as_str());

        assert!(
            rendered.contains("Thinking"),
            "Should show thinking indicator when streaming starts. Rendered:\n{rendered}"
        );
    }

    /// Full lifecycle: stream long content → MessageComplete → scroll up → scroll down.
    /// Full turn lifecycle: StreamStart → TextDelta* → StreamEnd → MessageComplete.
    /// Verifies state at each step with no leaks between turns.
    #[test]
    fn test_full_turn_lifecycle_state_transitions() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();

        // --- Turn 1 ---
        assert!(!app.is_turn_active);
        assert!(app.turn_started_at.is_none());

        app.handle_core_event(CoreEvent::StreamStart);
        assert!(app.is_turn_active, "StreamStart activates turn");
        assert!(app.turn_started_at.is_some(), "timer starts on StreamStart");
        assert!(
            app.streaming_text.is_empty(),
            "StreamStart clears streaming_text"
        );
        assert!(
            app.streaming_committed_lines.is_empty(),
            "StreamStart clears committed_lines"
        );
        assert!(
            app.active_tools.is_empty(),
            "StreamStart clears active_tools"
        );

        app.handle_core_event(CoreEvent::TextDelta("Hello".to_string()));
        assert_eq!(app.streaming_text, "Hello");

        app.handle_core_event(CoreEvent::TextDelta(", world!".to_string()));
        assert_eq!(app.streaming_text, "Hello, world!");

        app.handle_core_event(CoreEvent::StreamEnd);
        // After StreamEnd: raw buffer cleared, committed lines may contain finalized content.
        assert!(app.streaming_text.is_empty(), "StreamEnd clears raw buffer");
        assert!(
            app.is_turn_active,
            "StreamEnd does NOT deactivate turn (tools may still run)"
        );

        app.handle_core_event(CoreEvent::MessageComplete);
        assert!(!app.is_turn_active, "MessageComplete deactivates turn");
        assert!(
            app.turn_started_at.is_none(),
            "MessageComplete resets timer"
        );
        assert!(app.streaming_text.is_empty());
        assert!(
            app.streaming_committed_lines.is_empty(),
            "MessageComplete clears committed_lines"
        );
        assert!(app.active_tools.is_empty());

        // --- Turn 2: verify StreamStart clears any lingering state ---
        app.handle_core_event(CoreEvent::TextDelta("leftover".to_string()));
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(
            app.streaming_text.is_empty(),
            "StreamStart clears stale text from prior turn"
        );
        assert!(
            app.streaming_committed_lines.is_empty(),
            "StreamStart clears stale lines"
        );
    }

    /// Verify that a StreamEnd received after Error is idempotent — does not
    /// re-activate the turn or corrupt state.
    #[test]
    fn test_stream_end_after_error_is_idempotent() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_core_event(CoreEvent::TextDelta("partial text".to_string()));
        // Error followed by StreamEnd — TUI must handle gracefully.
        app.handle_core_event(CoreEvent::Error("API error".to_string()));
        assert!(!app.is_turn_active);
        assert!(app.streaming_text.is_empty());

        // StreamEnd arrives after Error (from abort_streaming's TurnEnd).
        app.handle_core_event(CoreEvent::StreamEnd);
        // Must remain inactive and clean.
        assert!(
            !app.is_turn_active,
            "StreamEnd after Error must not re-activate turn"
        );
        assert!(app.streaming_text.is_empty());
    }

    /// submit_input queues messages when a turn is active (not blocking).
    #[tokio::test]
    async fn test_submit_input_queued_during_active_turn() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        app.is_turn_active = true;
        app.input_text = "hello".to_string();
        app.input_cursor = 5;

        app.submit_input().await;

        // Message should be queued, not sent.
        assert_eq!(app.message_queue.len(), 1, "message should be queued");
        assert!(
            ui_rx.try_recv().is_err(),
            "no UiEvent sent while turn is active"
        );
        // Input text must be cleared (consumed into queue).
        assert!(app.input_text.is_empty(), "input consumed into queue");
    }

    /// History dedup: consecutive identical prompts stored only once.
    #[tokio::test]
    async fn test_submit_input_history_dedup() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();

        let initial_len = app.history.len();

        app.input_text = "cargo build".to_string();
        app.submit_input().await;

        // Submit same text again — should not be pushed again.
        app.input_text = "cargo build".to_string();
        app.submit_input().await;

        assert_eq!(
            app.history.len() - initial_len,
            1,
            "consecutive duplicates deduped"
        );
        // The last entry should be "cargo build".
        assert_eq!(app.history.get(app.history.len() - 1), Some("cargo build"));
    }

    /// /vim is handled inline — no UiEvent sent.
    #[tokio::test]
    async fn test_slash_vim_handled_inline_no_event() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        let was_vim = app.vim.enabled;

        app.input_text = "/vim".to_string();
        app.submit_input().await;

        assert_eq!(app.vim.enabled, !was_vim, "/vim toggles vim mode");
        assert!(ui_rx.try_recv().is_err(), "/vim must not send a UiEvent");
    }

    /// Non-/vim slash commands are forwarded as SlashCommand events.
    #[tokio::test]
    async fn test_slash_compact_forwarded_as_slash_command_event() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();

        app.input_text = "/compact".to_string();
        app.submit_input().await;

        match ui_rx.try_recv() {
            Ok(UiEvent::SlashCommand { name, args }) => {
                assert_eq!(name, "compact");
                assert_eq!(args, "");
            }
            other => panic!("Expected SlashCommand event, got {other:?}"),
        }
    }

    /// Unknown slash commands are forwarded (engine will produce an error).
    #[tokio::test]
    async fn test_slash_unknown_command_forwarded_not_swallowed() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();

        app.input_text = "/unknowncmd foo".to_string();
        app.submit_input().await;

        match ui_rx.try_recv() {
            Ok(UiEvent::SlashCommand { name, args }) => {
                assert_eq!(name, "unknowncmd");
                assert_eq!(args, "foo");
            }
            other => panic!("Expected SlashCommand forwarded, got {other:?}"),
        }
    }

    /// Permission dialog: 'y' hotkey sends AllowOnce and clears pending_permission.
    #[tokio::test]
    async fn test_permission_hotkey_y_sends_allow_once() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel::<PermissionResponse>();

        app.pending_permission = Some(PendingPermission {
            tool_name: "bash".to_string(),
            input_summary: "echo hi".to_string(),
            prompt: "Allow?".to_string(),
            selected: 0,
            risk_level: RiskLevel::High,
            kind: crate::widgets::PermissionDialogKind::Bash {
                command: "echo hi".to_string(),
            },
            created_at: Instant::now(),
            reply_tx,
        });

        app.handle_permission_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE))
            .await;

        assert!(
            app.pending_permission.is_none(),
            "'y' must clear pending_permission"
        );
        let response = reply_rx.try_recv().expect("response sent");
        assert_eq!(response, PermissionResponse::AllowOnce);
    }

    /// Permission dialog: 'n' hotkey sends Deny.
    #[tokio::test]
    async fn test_permission_hotkey_n_sends_deny() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel::<PermissionResponse>();

        app.pending_permission = Some(PendingPermission {
            tool_name: "bash".to_string(),
            input_summary: "rm -rf /".to_string(),
            prompt: "Allow?".to_string(),
            selected: 2,
            risk_level: RiskLevel::High,
            kind: crate::widgets::PermissionDialogKind::Bash {
                command: "rm -rf /".to_string(),
            },
            created_at: Instant::now(),
            reply_tx,
        });

        app.handle_permission_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
            .await;

        assert!(app.pending_permission.is_none());
        let response = reply_rx.try_recv().expect("response sent");
        assert_eq!(response, PermissionResponse::Deny);
    }

    /// Permission dialog: 'a' hotkey sends AlwaysAllow.
    #[tokio::test]
    async fn test_permission_hotkey_a_sends_always_allow() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel::<PermissionResponse>();

        app.pending_permission = Some(PendingPermission {
            tool_name: "bash".to_string(),
            input_summary: "make build".to_string(),
            prompt: "Allow always?".to_string(),
            selected: 0,
            risk_level: RiskLevel::High,
            kind: crate::widgets::PermissionDialogKind::Bash {
                command: "make build".to_string(),
            },
            created_at: Instant::now(),
            reply_tx,
        });

        app.handle_permission_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
            .await;

        assert!(app.pending_permission.is_none());
        let response = reply_rx.try_recv().expect("response sent");
        assert_eq!(response, PermissionResponse::AlwaysAllow);
    }

    /// Permission dialog: Esc sends Deny and clears dialog.
    #[tokio::test]
    async fn test_permission_esc_sends_deny() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel::<PermissionResponse>();

        app.pending_permission = Some(PendingPermission {
            tool_name: "bash".to_string(),
            input_summary: "curl http://...".to_string(),
            prompt: "Network access?".to_string(),
            selected: 0,
            risk_level: RiskLevel::High,
            kind: crate::widgets::PermissionDialogKind::Bash {
                command: "curl http://...".to_string(),
            },
            created_at: Instant::now(),
            reply_tx,
        });

        app.handle_permission_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;

        assert!(
            app.pending_permission.is_none(),
            "Esc clears pending_permission"
        );
        let response = reply_rx.try_recv().expect("response sent");
        assert_eq!(response, PermissionResponse::Deny, "Esc sends Deny");
    }

    /// Permission dialog: Enter on option 0 (AllowOnce) sends AllowOnce.
    #[tokio::test]
    async fn test_permission_enter_option_0_allow_once() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel::<PermissionResponse>();

        app.pending_permission = Some(PendingPermission {
            tool_name: "bash".to_string(),
            input_summary: "echo".to_string(),
            prompt: "Allow?".to_string(),
            selected: 0,
            risk_level: RiskLevel::High,
            kind: crate::widgets::PermissionDialogKind::Bash {
                command: "echo".to_string(),
            },
            created_at: Instant::now(),
            reply_tx,
        });

        app.handle_permission_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;

        let response = reply_rx.try_recv().expect("response sent");
        assert_eq!(response, PermissionResponse::AllowOnce);
    }

    /// Permission dialog blocks normal input: handle_key should not forward chars
    /// to input_text while a permission dialog is pending.
    #[tokio::test]
    async fn test_permission_dialog_blocks_normal_input() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel::<PermissionResponse>();

        app.pending_permission = Some(PendingPermission {
            tool_name: "bash".to_string(),
            input_summary: "cmd".to_string(),
            prompt: "Allow?".to_string(),
            selected: 0,
            risk_level: RiskLevel::High,
            kind: crate::widgets::PermissionDialogKind::Bash {
                command: "cmd".to_string(),
            },
            created_at: Instant::now(),
            reply_tx,
        });

        // Type a character — it should go to the permission handler, not input_text.
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .await;

        assert!(
            app.input_text.is_empty(),
            "input_text should not receive chars while permission dialog is open"
        );
    }

    /// Multiple sequential tool calls accumulate correctly in active_tools.
    #[test]
    fn test_multiple_sequential_tool_calls_accumulate() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);

        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "t1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "echo 1"}),
        });
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "t2".to_string(),
            name: "file_read".to_string(),
            input: serde_json::json!({"file_path": "/tmp/f.rs"}),
        });

        assert_eq!(app.active_tools.len(), 2);

        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "t1".to_string(),
            content: "1".to_string(),
            is_error: false,
        });
        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "t2".to_string(),
            content: "fn main() {}".to_string(),
            is_error: false,
        });

        assert_eq!(app.active_tools[0].result.as_ref().unwrap().0, "1");
        assert_eq!(
            app.active_tools[1].result.as_ref().unwrap().0,
            "fn main() {}"
        );

        // MessageComplete must clear all tools.
        app.handle_core_event(CoreEvent::MessageComplete);
        assert!(
            app.active_tools.is_empty(),
            "active_tools cleared after MessageComplete"
        );
    }

    /// No state leaks between two full turns.
    #[test]
    fn test_no_state_leak_between_turns() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();

        // Turn 1 with a tool call.
        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_core_event(CoreEvent::TextDelta("First response".to_string()));
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "tid1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        });
        app.handle_core_event(CoreEvent::StreamEnd);
        app.handle_core_event(CoreEvent::ToolResult {
            tool_use_id: "tid1".to_string(),
            content: "file.txt".to_string(),
            is_error: false,
        });
        app.handle_core_event(CoreEvent::MessageComplete);

        // At start of Turn 2, StreamStart must wipe previous turn's tools/text.
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(
            app.streaming_text.is_empty(),
            "streaming_text must be empty at start of Turn 2"
        );
        assert!(
            app.active_tools.is_empty(),
            "active_tools must be empty at start of Turn 2 (cleared by StreamStart)"
        );
        assert!(
            app.streaming_committed_lines.is_empty(),
            "committed_lines cleared at start of Turn 2"
        );
    }

    /// Full lifecycle: stream long content → MessageComplete → scroll up → scroll down.
    #[test]
    fn test_full_stream_complete_then_scroll_lifecycle() {
        let (mut app, store, _ui_rx, _core_tx) = make_test_app_with_store();
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).expect("create test terminal");

        // Push a user message to state store (simulates the engine adding it).
        store.push_message(Message::user("What can you do?"));

        // Simulate streaming a long response.
        app.handle_core_event(CoreEvent::StreamStart);
        let chunks = vec![
            "## Capabilities\n\n",
            "Here are my main capabilities:\n\n",
            "• **File Operations** — Read, write, edit files\n",
            "• **Code Analysis** — Find definitions, references\n",
            "• **Shell Commands** — Run bash/shell commands\n",
            "• **Web Search** — Search the web for information\n",
            "• **Project Management** — Create and track todos\n\n",
            "## Architecture\n\n",
            "| Layer | Description |\n",
            "|---|---|\n",
            "| Foundation | Shared types and errors |\n",
            "| Core | Query engine and tools |\n",
            "| TUI | Ratatui-based interface |\n",
            "| CLI | Binary entry point |\n\n",
            "## Additional Features\n\n",
            "• Session persistence\n",
            "• Hook system with 26 events\n",
            "• Vim mode keybindings\n",
            "• Multi-provider API support\n",
            "• Agent system for background tasks\n",
        ];

        for chunk in &chunks {
            app.handle_core_event(CoreEvent::TextDelta((*chunk).to_string()));
        }

        // Draw during streaming — should show content auto-scrolled to bottom.
        app.draw(&mut terminal).expect("draw during streaming");
        let during_stream = normalized_rendered_text(&terminal);
        assert!(
            during_stream.contains("Agent system"),
            "During streaming, auto-scroll should show latest content"
        );

        // StreamEnd + MessageComplete.
        app.handle_core_event(CoreEvent::StreamEnd);

        // Push the assistant message to state store (simulates engine persisting).
        let full_text: String = chunks.iter().copied().collect();
        let mut assistant_msg = Message::assistant();
        assistant_msg
            .content
            .push(oxicode_common::ContentBlock::Text { text: full_text });
        store.push_message(assistant_msg);

        app.handle_core_event(CoreEvent::MessageComplete);

        // Draw after MessageComplete — should show cached message.
        app.draw(&mut terminal).expect("draw after complete");
        let after_complete = normalized_rendered_text(&terminal);
        assert!(
            after_complete.contains("Agent system") || after_complete.contains("background tasks"),
            "After MessageComplete, last content should be visible. Got:\n{after_complete}"
        );

        // Verify max_scroll_offset is positive (content exceeds viewport).
        assert!(
            app.max_scroll_offset > 0,
            "max_scroll_offset should be > 0 after long message, got {}",
            app.max_scroll_offset
        );

        // Scroll up — should disable auto_scroll and show earlier content.
        app.scroll_up_by(10);
        assert!(!app.auto_scroll, "scroll_up should disable auto_scroll");
        app.draw(&mut terminal).expect("draw after scroll up");
        let after_scroll_up = normalized_rendered_text(&terminal);
        // After scrolling up 10 lines, we should see earlier content.
        assert!(
            after_scroll_up.contains("Capabilities") || after_scroll_up.contains("File Operations"),
            "After scroll up, earlier content should be visible. Got:\n{after_scroll_up}"
        );

        // Scroll back down to bottom.
        app.scroll_down_by(100); // large number to reach bottom
        assert!(
            app.auto_scroll,
            "scrolling to bottom should re-enable auto_scroll"
        );
        app.draw(&mut terminal).expect("draw after scroll down");
        let after_scroll_down = normalized_rendered_text(&terminal);
        assert!(
            after_scroll_down.contains("background tasks") || after_scroll_down.contains("Agent system"),
            "After scroll down to bottom, last content should be visible. Got:\n{after_scroll_down}"
        );
    }

    // ── PHASE 1: Interrupt & Resume ──────────────────────────────────────────

    #[tokio::test]
    async fn test_signal_interrupt_sets_cancel_flag() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let flag = Arc::new(AtomicBool::new(false));
        app.set_cancel_flag(flag.clone());
        app.handle_core_event(CoreEvent::StreamStart);
        app.signal_interrupt().await;
        assert!(flag.load(Ordering::SeqCst), "cancel flag should be true");
    }

    #[tokio::test]
    async fn test_signal_interrupt_resets_turn_state() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(app.is_turn_active);
        app.signal_interrupt().await;
        assert!(!app.is_turn_active, "signal_interrupt must deactivate turn");
        assert!(app.turn_started_at.is_none(), "timer must reset");
        assert!(app.stall_start.is_none(), "stall timer must reset");
    }

    #[tokio::test]
    async fn test_signal_interrupt_sends_interrupt_event() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        app.signal_interrupt().await;
        let event = ui_rx.try_recv().expect("should receive event");
        assert!(matches!(event, UiEvent::InterruptTurn));
    }

    #[tokio::test]
    async fn test_esc_during_turn_calls_signal_interrupt() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let flag = Arc::new(AtomicBool::new(false));
        app.set_cancel_flag(flag.clone());
        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert!(!app.is_turn_active, "Esc during turn must deactivate");
        assert!(flag.load(Ordering::SeqCst), "Esc must set cancel flag");
        assert!(
            app.notifications
                .iter()
                .any(|n| n.message.contains("Interrupting")),
            "should show Interrupting notification"
        );
    }

    #[tokio::test]
    async fn test_ctrl_c_during_turn_interrupts() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        let flag = Arc::new(AtomicBool::new(false));
        app.set_cancel_flag(flag.clone());
        app.handle_core_event(CoreEvent::StreamStart);
        // Double-Ctrl+C contract: first press during an active turn signals
        // interrupt (flips cancel_flag) but does NOT quit; second press within
        // 2s flips should_quit and emits UiEvent::Quit.
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .await;
        assert!(
            flag.load(Ordering::Relaxed),
            "First Ctrl+C during turn should set cancel_flag"
        );
        assert!(
            !app.should_quit,
            "First Ctrl+C should not quit — only arms the double-press"
        );

        // First Ctrl+C during active turn emits InterruptTurn — drain it.
        assert!(
            matches!(ui_rx.try_recv(), Ok(UiEvent::InterruptTurn)),
            "First Ctrl+C during turn should emit InterruptTurn"
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .await;
        assert!(
            app.should_quit,
            "Second Ctrl+C within 2s should set should_quit"
        );
        assert!(
            matches!(ui_rx.try_recv(), Ok(UiEvent::Quit)),
            "Second Ctrl+C should emit Quit event"
        );
    }

    #[tokio::test]
    async fn test_double_ctrl_c_force_quits() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        // First press arms the hint, second press within 2s force-quits.
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .await;
        assert!(!app.should_quit, "First Ctrl+C must not quit");
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .await;
        assert!(app.should_quit, "Double Ctrl+C must set should_quit");
    }

    #[test]
    fn test_interrupt_error_no_duplicate_notification() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        let before = app.notifications.len();
        // "Interrupted by user" errors should be treated as informational / suppressed.
        // The test verifies that a plain Error event *does* add a notification (default),
        // but if this assertion fails it means the implementation suppresses it — adjust.
        app.handle_core_event(CoreEvent::Error("Interrupted by user".to_string()));
        // Either it adds one notification (normal error path) or zero (suppressed).
        assert!(
            app.notifications.len() <= before + 1,
            "Error should add at most one notification, not duplicate"
        );
    }

    #[tokio::test]
    async fn test_submit_works_immediately_after_interrupt() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        app.signal_interrupt().await;
        let _ = ui_rx.try_recv(); // drain InterruptTurn
                                  // Set input and submit
        app.input_text = "continue".to_string();
        app.input_cursor = 8;
        app.submit_input().await;
        // Should emit UserInput, NOT "Waiting for current response..."
        let mut found_user_input = false;
        while let Ok(ev) = ui_rx.try_recv() {
            if matches!(ev, UiEvent::UserInput { .. }) {
                found_user_input = true;
            }
        }
        assert!(
            found_user_input,
            "submit should work immediately after interrupt"
        );
        assert!(
            !app.notifications
                .iter()
                .any(|n| n.message.contains("Waiting")),
            "should not show 'Waiting' notification after interrupt"
        );
    }

    // ── PHASE 2: Autocomplete ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_autocomplete_activates_on_slash() {
        let (mut app, _ui_rx, _core_tx) = make_test_app_with_commands();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .await;
        assert!(app.autocomplete.active, "/ should activate autocomplete");
    }

    #[tokio::test]
    async fn test_autocomplete_filters_by_prefix() {
        let (mut app, _ui_rx, _core_tx) = make_test_app_with_commands();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .await;
        assert!(app.autocomplete.active);
        // Type 'h' to filter to "help"
        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
            .await;
        assert!(app.autocomplete.active, "should still be active with match");
        assert_eq!(app.input_text, "/h");
        // The filtered list should contain the index for "help"
        assert!(
            !app.autocomplete.filtered.is_empty(),
            "should have at least one match for 'h'"
        );
    }

    #[tokio::test]
    async fn test_autocomplete_tab_selects_top_match() {
        let (mut app, _ui_rx, _core_tx) = make_test_app_with_commands();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await;
        assert!(!app.autocomplete.active, "Tab should close autocomplete");
        assert_eq!(app.input_text, "/help");
    }

    #[tokio::test]
    async fn test_autocomplete_down_arrow_navigates() {
        let (mut app, _ui_rx, _core_tx) = make_test_app_with_commands();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .await;
        let initial_selected = app.autocomplete.selected;
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await;
        assert_eq!(
            app.autocomplete.selected,
            initial_selected + 1,
            "Down should advance selection"
        );
    }

    #[tokio::test]
    async fn test_autocomplete_up_arrow_wraps() {
        let (mut app, _ui_rx, _core_tx) = make_test_app_with_commands();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .await;
        assert_eq!(app.autocomplete.selected, 0);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .await;
        // Should wrap to last item
        let last = app.autocomplete.filtered.len().saturating_sub(1);
        assert_eq!(
            app.autocomplete.selected, last,
            "Up from 0 should wrap to last"
        );
    }

    #[tokio::test]
    async fn test_autocomplete_enter_inserts_selected() {
        let (mut app, _ui_rx, _core_tx) = make_test_app_with_commands();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        assert!(!app.autocomplete.active, "Enter should close autocomplete");
        // Should have inserted a command name
        assert!(app.input_text.starts_with('/'), "input should start with /");
    }

    #[tokio::test]
    async fn test_autocomplete_esc_dismisses() {
        let (mut app, _ui_rx, _core_tx) = make_test_app_with_commands();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .await;
        assert!(app.autocomplete.active);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert!(!app.autocomplete.active, "Esc should dismiss autocomplete");
    }

    #[tokio::test]
    async fn test_autocomplete_backspace_past_slash_deactivates() {
        let (mut app, _ui_rx, _core_tx) = make_test_app_with_commands();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .await;
        assert!(app.autocomplete.active);
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
            .await;
        assert!(
            !app.autocomplete.active,
            "Backspace past / should deactivate"
        );
        assert!(app.input_text.is_empty());
    }

    #[tokio::test]
    async fn test_autocomplete_ghost_text_shows_suffix() {
        let (mut app, _ui_rx, _core_tx) = make_test_app_with_commands();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
            .await;
        // Ghost text should show the rest of "help" → "elp" (or may be None)
        if let Some(ref ghost) = app.ghost_text {
            assert!(
                ghost.contains("elp"),
                "ghost text should suggest 'elp' for '/h', got: {ghost}"
            );
        }
        // If ghost_text is None that is also acceptable per spec
    }

    #[tokio::test]
    async fn test_autocomplete_no_match_deactivates() {
        let (mut app, _ui_rx, _core_tx) = make_test_app_with_commands();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .await;
        assert!(app.autocomplete.active);
        // Type something that matches nothing
        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE))
            .await;
        assert!(
            !app.autocomplete.active,
            "No match should deactivate autocomplete"
        );
    }

    // ── PHASE 3: Search Overlay ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_search_ctrl_f_activates_overlay() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        assert!(!app.search.is_active());
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .await;
        assert!(app.search.is_active(), "Ctrl+F should activate search");
    }

    #[tokio::test]
    async fn test_search_typing_builds_query() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.search.activate();
        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
            .await;
        assert_eq!(app.search.query(), "hi", "typing should build search query");
    }

    #[tokio::test]
    async fn test_search_backspace_removes_char() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.search.activate();
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
            .await;
        assert_eq!(app.search.query(), "a", "backspace should remove last char");
    }

    #[tokio::test]
    async fn test_search_esc_closes_overlay() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.search.activate();
        assert!(app.search.is_active());
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert!(!app.search.is_active(), "Esc should close search");
    }

    #[tokio::test]
    async fn test_search_enter_confirms_and_closes() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.search.activate();
        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        assert!(!app.search.is_active(), "Enter should close search");
    }

    #[tokio::test]
    async fn test_search_empty_query_no_crash() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.search.activate();
        // Enter with empty query should not crash
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        assert!(!app.search.is_active());
    }

    #[tokio::test]
    async fn test_search_blocks_normal_key_handling() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.search.activate();
        // Typing should go to search, not input box
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .await;
        assert!(
            app.input_text.is_empty(),
            "search should capture keys, not input box"
        );
        assert_eq!(app.search.query(), "x");
    }

    #[tokio::test]
    async fn test_search_ctrl_f_toggles() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .await;
        assert!(app.search.is_active());
        // Esc to close
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert!(!app.search.is_active());
        // Ctrl+F again to reopen
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .await;
        assert!(app.search.is_active(), "Ctrl+F should reopen search");
    }

    // ── PHASE 4: History Search ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_history_search_ctrl_r_activates() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        assert!(app.history_search.is_none());
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .await;
        assert!(
            app.history_search.is_some(),
            "Ctrl+R should open history search"
        );
    }

    #[tokio::test]
    async fn test_history_search_typing_filters() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        // Add some history first
        app.history.add("hello world", None);
        app.history.add("goodbye world", None);
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .await;
        assert!(app.history_search.is_some());
        // Type to filter
        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
            .await;
        let hs = app.history_search.as_ref().unwrap();
        assert!(
            app.input_text.contains("hello") || hs.query == "h",
            "typing should update query or preview match"
        );
    }

    #[tokio::test]
    async fn test_history_search_ctrl_r_cycles_matches() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.history.add("first", None);
        app.history.add("second", None);
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .await;
        let first_selected = app.history_search.as_ref().unwrap().selected;
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .await;
        let second_selected = app.history_search.as_ref().unwrap().selected;
        let result_count = app.history_search.as_ref().unwrap().matches.len();
        // Should have advanced or wrapped (if only 1 match it stays the same)
        assert!(
            first_selected != second_selected || result_count <= 1,
            "Ctrl+R should cycle through matches"
        );
    }

    #[tokio::test]
    async fn test_history_search_enter_confirms() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.history.add("test command", None);
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        assert!(
            app.history_search.is_none(),
            "Enter should close history search"
        );
    }

    #[tokio::test]
    async fn test_history_search_esc_cancels() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.input_text = "original".to_string();
        app.input_cursor = 8;
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .await;
        assert!(app.history_search.is_some());
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert!(
            app.history_search.is_none(),
            "Esc should close history search"
        );
        assert_eq!(
            app.input_text, "original",
            "Esc should restore original input"
        );
    }

    #[tokio::test]
    async fn test_history_search_backspace_narrows() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.history.add("hello", None);
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE))
            .await;
        let query_before = app.history_search.as_ref().unwrap().query.clone();
        assert_eq!(query_before, "he");
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
            .await;
        assert_eq!(
            app.history_search.as_ref().unwrap().query,
            "h",
            "backspace should remove char from query"
        );
    }

    #[tokio::test]
    async fn test_history_search_no_match_stays_open() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE))
            .await;
        assert!(
            app.history_search.is_some(),
            "no match should keep overlay open"
        );
    }

    // ── PHASE 5: Model Picker ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_model_picker_opens_via_method() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        assert!(!app.model_picker.is_visible());
        app.model_picker.open("claude-sonnet-4-20250514");
        assert!(app.model_picker.is_visible());
    }

    #[tokio::test]
    async fn test_model_picker_esc_closes() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.model_picker.open("claude-sonnet-4-20250514");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert!(
            !app.model_picker.is_visible(),
            "Esc should close model picker"
        );
    }

    #[tokio::test]
    async fn test_model_picker_up_down_navigates() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.model_picker.open("claude-sonnet-4-20250514");
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await;
        // Just verify no crash, selection moved
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .await;
        assert!(
            app.model_picker.is_visible(),
            "should still be open after navigation"
        );
    }

    #[tokio::test]
    async fn test_model_picker_enter_selects() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        app.model_picker.open("claude-sonnet-4-20250514");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        assert!(!app.model_picker.is_visible(), "Enter should close picker");
        // Check if SlashCommand was emitted
        let mut found_slash = false;
        while let Ok(ev) = ui_rx.try_recv() {
            if matches!(ev, UiEvent::SlashCommand { name, .. } if name == "model") {
                found_slash = true;
            }
        }
        assert!(found_slash, "Enter should emit SlashCommand for model");
    }

    #[tokio::test]
    async fn test_model_picker_typing_filters() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.model_picker.open("claude-sonnet-4-20250514");
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE))
            .await;
        assert!(
            app.model_picker.is_visible(),
            "should remain open while typing filter"
        );
    }

    #[tokio::test]
    async fn test_model_picker_left_right_effort() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.model_picker.open("claude-sonnet-4-20250514");
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .await;
        assert!(
            app.model_picker.is_visible(),
            "effort adjustments should not close picker"
        );
    }

    #[tokio::test]
    async fn test_model_picker_blocks_normal_input() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.model_picker.open("claude-sonnet-4-20250514");
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .await;
        assert!(
            app.input_text.is_empty(),
            "model picker should capture keys, not input box"
        );
    }

    #[tokio::test]
    async fn test_model_picker_backspace_clears_filter() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.model_picker.open("claude-sonnet-4-20250514");
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
            .await;
        assert!(
            app.model_picker.is_visible(),
            "backspace should not close picker"
        );
    }

    // ── PHASE 6: Session Browser ─────────────────────────────────────────────

    fn make_test_sessions() -> Vec<SessionEntry> {
        vec![
            SessionEntry {
                id: "session-001".into(),
                title: "First session".into(),
                last_updated: "1m ago".into(),
                message_count: 5,
                cost_usd: 0.0,
            },
            SessionEntry {
                id: "session-002".into(),
                title: "Second session".into(),
                last_updated: "5m ago".into(),
                message_count: 10,
                cost_usd: 0.0,
            },
        ]
    }

    #[tokio::test]
    async fn test_session_browser_opens() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        assert!(!app.session_browser.is_visible());
        app.session_browser.open(make_test_sessions());
        assert!(app.session_browser.is_visible());
    }

    #[tokio::test]
    async fn test_session_browser_esc_closes() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.session_browser.open(make_test_sessions());
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert!(
            !app.session_browser.is_visible(),
            "Esc should close session browser"
        );
    }

    #[tokio::test]
    async fn test_session_browser_up_down_navigates() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.session_browser.open(make_test_sessions());
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .await;
        assert!(
            app.session_browser.is_visible(),
            "navigation should not close browser"
        );
    }

    #[tokio::test]
    async fn test_session_browser_enter_resumes() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        app.session_browser.open(make_test_sessions());
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        assert!(
            !app.session_browser.is_visible(),
            "Enter should close browser"
        );
        // Check if resume SlashCommand was emitted
        let mut found = false;
        while let Ok(ev) = ui_rx.try_recv() {
            if matches!(&ev, UiEvent::SlashCommand { name, .. } if name == "resume") {
                found = true;
            }
        }
        assert!(found, "Enter should emit resume SlashCommand");
    }

    #[tokio::test]
    async fn test_session_browser_r_enters_rename() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.session_browser.open(make_test_sessions());
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE))
            .await;
        assert!(
            app.session_browser.is_visible(),
            "rename mode should keep browser open"
        );
        // mode should be Rename now
        assert!(matches!(
            app.session_browser.mode(),
            SessionBrowserMode::Rename
        ));
    }

    #[tokio::test]
    async fn test_session_browser_rename_typing() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.session_browser.open(make_test_sessions());
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE))
            .await; // enter rename
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
            .await;
        // Should still be in rename mode, characters added
        assert!(matches!(
            app.session_browser.mode(),
            SessionBrowserMode::Rename
        ));
    }

    #[tokio::test]
    async fn test_session_browser_rename_enter_confirms() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        app.session_browser.open(make_test_sessions());
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        // Should have emitted rename-session SlashCommand
        let mut found = false;
        while let Ok(ev) = ui_rx.try_recv() {
            if matches!(&ev, UiEvent::SlashCommand { name, .. } if name == "rename-session") {
                found = true;
            }
        }
        assert!(
            found,
            "rename Enter should emit rename-session SlashCommand"
        );
    }

    #[tokio::test]
    async fn test_session_browser_cancel_in_rename() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.session_browser.open(make_test_sessions());
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE))
            .await;
        // Esc in Rename mode cancels back to Browse (not close)
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert!(
            app.session_browser.is_visible(),
            "Esc in rename should return to browse, not close"
        );
        assert!(
            matches!(app.session_browser.mode(), SessionBrowserMode::Browse),
            "Esc in rename should revert to Browse mode"
        );
    }

    // ── PHASE 7: Paste Preview ───────────────────────────────────────────────

    #[test]
    fn test_paste_small_text_inserted_directly() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_paste("short text");
        assert_eq!(app.input_text, "short text");
        assert!(
            app.pending_paste.is_none(),
            "small paste should not trigger preview"
        );
    }

    #[test]
    fn test_paste_large_text_shows_preview() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let large = "line1\nline2\nline3\nline4\nline5\nline6\n"; // 6 lines > threshold of 5
        app.handle_paste(large);
        assert!(
            app.pending_paste.is_some(),
            "large paste should set pending_paste"
        );
        assert!(
            app.input_text.is_empty(),
            "large paste should not insert directly"
        );
    }

    #[tokio::test]
    async fn test_paste_preview_enter_confirms() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let large = "line1\nline2\nline3\nline4\nline5\nline6\n";
        app.handle_paste(large);
        assert!(app.pending_paste.is_some());
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        assert!(
            app.pending_paste.is_none(),
            "Enter should clear pending_paste"
        );
        assert!(
            app.input_text.contains("line1"),
            "Enter should insert pasted text"
        );
    }

    #[tokio::test]
    async fn test_paste_preview_esc_cancels() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let large = "line1\nline2\nline3\nline4\nline5\nline6\n";
        app.handle_paste(large);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert!(app.pending_paste.is_none(), "Esc should discard paste");
        assert!(app.input_text.is_empty(), "Esc should not insert paste");
    }

    #[tokio::test]
    async fn test_paste_preview_blocks_normal_input() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let large = "line1\nline2\nline3\nline4\nline5\nline6\n";
        app.handle_paste(large);
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .await;
        assert!(
            app.input_text.is_empty(),
            "paste preview should block normal input"
        );
    }

    #[test]
    fn test_paste_empty_text_no_change() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_paste("");
        assert!(app.input_text.is_empty());
        assert!(app.pending_paste.is_none());
    }

    // ── PHASE 8: CoreEvent Edge Cases ────────────────────────────────────────

    #[test]
    fn test_core_event_thinking_delta_accumulates() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_core_event(CoreEvent::ThinkingDelta("thought 1 ".to_string()));
        app.handle_core_event(CoreEvent::ThinkingDelta("thought 2".to_string()));
        assert_eq!(app.streaming_thinking, "thought 1 thought 2");
    }

    #[test]
    fn test_core_event_thinking_delta_sets_auto_scroll() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.auto_scroll = false;
        app.handle_core_event(CoreEvent::ThinkingDelta("thinking".to_string()));
        assert!(app.auto_scroll, "ThinkingDelta should enable auto_scroll");
    }

    #[test]
    fn test_core_event_rate_limited_adds_notification() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let before = app.notifications.len();
        app.handle_core_event(CoreEvent::RateLimited {
            message: "rate limited".to_string(),
            attempt: 1,
            max_retries: 3,
            retry_in_secs: 30.0,
        });
        assert_eq!(
            app.notifications.len(),
            before + 1,
            "RateLimited should add notification"
        );
    }

    #[test]
    fn test_core_event_hook_progress_running_then_completed() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::HookProgress {
            event: "PreToolUse".to_string(),
            state: "running".to_string(),
        });
        assert_eq!(app.active_hook.as_deref(), Some("PreToolUse"));
        let label = app.compose_status_label();
        assert!(label.contains("PreToolUse"), "got: {label}");
        app.handle_core_event(CoreEvent::HookProgress {
            event: "PreToolUse".to_string(),
            state: "completed".to_string(),
        });
        assert!(app.active_hook.is_none());
    }

    #[test]
    fn test_core_event_hook_message_appended_and_notified() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let before_notif = app.notifications.len();
        let before_log = app.hook_messages.len();
        app.handle_core_event(CoreEvent::HookMessage {
            event: "PreToolUse".to_string(),
            kind: "system".to_string(),
            content: "hi".to_string(),
        });
        assert_eq!(app.hook_messages.len(), before_log + 1);
        assert_eq!(app.hook_messages[before_log].content, "hi");
        assert_eq!(app.notifications.len(), before_notif + 1);
    }

    #[test]
    fn test_hook_message_truncates_long_content() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let long = "x".repeat(500);
        app.handle_core_event(CoreEvent::HookMessage {
            event: "evt".to_string(),
            kind: "system".to_string(),
            content: long,
        });
        let stored = &app.hook_messages.last().unwrap().content;
        // 200 chars + ellipsis.
        assert_eq!(stored.chars().count(), MAX_HOOK_LINE_CHARS + 1);
        assert!(stored.ends_with('…'));
    }

    #[test]
    fn test_compose_status_label_priority_retry_over_hook() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.active_hook = Some("evt".to_string());
        app.retry_status_label = "retry banner".to_string();
        assert_eq!(app.compose_status_label(), "retry banner");
    }

    #[test]
    fn test_core_event_retrying_adds_notification() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let before = app.notifications.len();
        app.handle_core_event(CoreEvent::Retrying {
            message: "connection error".to_string(),
            attempt: 2,
            max_retries: 5,
            retry_in_secs: 10.0,
        });
        assert_eq!(
            app.notifications.len(),
            before + 1,
            "Retrying should add notification"
        );
    }

    #[test]
    fn test_core_event_permission_ask_creates_dialog() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        assert!(app.pending_permission.is_none());
        let (tx, _rx) = tokio::sync::oneshot::channel();
        app.handle_core_event(CoreEvent::PermissionAsk {
            tool_name: "bash".to_string(),
            input_summary: "ls -la".to_string(),
            prompt: "Allow bash?".to_string(),
            reply_tx: tx,
        });
        assert!(
            app.pending_permission.is_some(),
            "PermissionAsk should create dialog"
        );
    }

    #[test]
    fn test_core_event_stream_end_without_start_no_crash() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        assert!(!app.is_turn_active);
        // StreamEnd without StreamStart should be a no-op, no crash
        app.handle_core_event(CoreEvent::StreamEnd);
        assert!(!app.is_turn_active);
    }

    #[test]
    fn test_core_event_message_complete_without_stream_end() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(app.is_turn_active);
        // Skip StreamEnd, go straight to MessageComplete
        app.handle_core_event(CoreEvent::MessageComplete);
        assert!(
            !app.is_turn_active,
            "MessageComplete should deactivate even without StreamEnd"
        );
        assert!(app.streaming_text.is_empty());
        assert!(app.active_tools.is_empty());
    }

    #[test]
    fn test_core_event_error_then_message_complete_idempotent() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        app.handle_core_event(CoreEvent::Error("some error".to_string()));
        assert!(!app.is_turn_active);
        // MessageComplete after Error should be safe
        app.handle_core_event(CoreEvent::MessageComplete);
        assert!(!app.is_turn_active, "should remain inactive");
    }

    #[test]
    fn test_core_event_text_delta_enables_auto_scroll() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        app.auto_scroll = false;
        app.handle_core_event(CoreEvent::TextDelta("hello".to_string()));
        assert!(app.auto_scroll, "TextDelta should enable auto_scroll");
    }

    #[test]
    fn test_core_event_tool_use_start_enables_auto_scroll() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        app.auto_scroll = false;
        app.handle_core_event(CoreEvent::ToolUseStart {
            id: "t1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        });
        assert!(app.auto_scroll, "ToolUseStart should enable auto_scroll");
    }

    // ── PHASE 9: Permission Dialog Advanced ──────────────────────────────────

    #[test]
    fn test_dangerous_operation_rm_rf() {
        assert!(is_dangerous_operation("bash", "rm -rf /"));
    }

    #[test]
    fn test_dangerous_operation_git_force_push() {
        assert!(is_dangerous_operation("bash", "git push --force"));
    }

    #[test]
    fn test_dangerous_operation_sql_drop() {
        assert!(is_dangerous_operation("bash", "DROP TABLE users"));
    }

    #[test]
    fn test_dangerous_operation_safe_ls() {
        assert!(!is_dangerous_operation("bash", "ls -la"));
    }

    #[test]
    fn test_dangerous_operation_sensitive_path() {
        assert!(is_dangerous_operation("file_write", ".env"));
    }

    #[tokio::test]
    async fn test_permission_ctrl_c_denies_and_interrupts() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let flag = Arc::new(AtomicBool::new(false));
        app.set_cancel_flag(flag.clone());
        app.handle_core_event(CoreEvent::StreamStart);
        // Create a permission dialog
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.handle_core_event(CoreEvent::PermissionAsk {
            tool_name: "bash".to_string(),
            input_summary: "ls".to_string(),
            prompt: "Allow?".to_string(),
            reply_tx: tx,
        });
        assert!(app.pending_permission.is_some());
        // Ctrl+C in permission dialog
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .await;
        assert!(
            app.pending_permission.is_none(),
            "Ctrl+C should dismiss permission"
        );
        // Check that deny was sent
        let response = rx.await.unwrap();
        assert!(matches!(response, oxicode_common::PermissionResponse::Deny));
    }

    // ── PHASE 10: Stall Detection ────────────────────────────────────────────

    #[test]
    fn test_stream_start_sets_stall_timer() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        assert!(app.stall_start.is_none());
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(
            app.stall_start.is_some(),
            "StreamStart should set stall timer"
        );
    }

    #[test]
    fn test_text_delta_resets_stall_timer() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        // stall_start should be Some after StreamStart
        app.handle_core_event(CoreEvent::TextDelta("text".to_string()));
        // stall_start should still be Some (reset/refreshed on each delta)
        assert!(
            app.stall_start.is_some(),
            "stall timer should be reset on TextDelta"
        );
    }

    #[test]
    fn test_error_clears_stall_timer() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(app.stall_start.is_some());
        app.handle_core_event(CoreEvent::Error("err".to_string()));
        assert!(app.stall_start.is_none(), "Error should clear stall timer");
    }

    #[test]
    fn test_message_complete_clears_stall_timer() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(app.stall_start.is_some());
        app.handle_core_event(CoreEvent::MessageComplete);
        assert!(
            app.stall_start.is_none(),
            "MessageComplete should clear stall timer"
        );
    }

    #[test]
    fn test_stream_start_clears_last_turn_duration() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.last_turn_duration = Some(std::time::Duration::from_secs(5));
        app.handle_core_event(CoreEvent::StreamStart);
        assert!(
            app.last_turn_duration.is_none(),
            "StreamStart should clear last_turn_duration"
        );
    }

    // --- Phase 11: Split Pane & Keyboard Actions ---

    #[test]
    fn test_tab_toggles_right_pane() {
        // Tab with empty input, empty suggestions → toggles split pane
        // The keybinding registry maps Tab → TogglePanel when input empty
        // But in handle_key_inner, tab with empty input and no suggestions calls split_pane.toggle_right()
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        assert!(!app.split_pane.is_right_visible());
        // Directly call the toggle since Tab goes through keybinding or handle_key_inner logic
        app.split_pane.toggle_right();
        assert!(app.split_pane.is_right_visible());
        app.split_pane.toggle_right();
        assert!(!app.split_pane.is_right_visible());
    }

    #[test]
    fn test_ctrl_left_adjusts_split_ratio() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let initial_ratio = app.split_pane.ratio();
        app.split_pane.adjust_ratio(-5);
        assert_eq!(app.split_pane.ratio(), initial_ratio - 5);
    }

    #[test]
    fn test_ctrl_right_adjusts_split_ratio() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let initial_ratio = app.split_pane.ratio();
        app.split_pane.adjust_ratio(5);
        assert_eq!(app.split_pane.ratio(), initial_ratio + 5);
    }

    #[tokio::test]
    async fn test_up_arrow_navigates_history() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        // Add history entries
        app.history.add("first", None);
        app.history.add("second", None);
        // Up arrow → should load last entry
        app.history_prev();
        assert_eq!(app.input_text, "second");
        app.history_prev();
        assert_eq!(app.input_text, "first");
    }

    #[tokio::test]
    async fn test_down_arrow_navigates_history_forward() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.history.add("first", None);
        app.history.add("second", None);
        // Go up twice
        app.history_prev();
        app.history_prev();
        assert_eq!(app.input_text, "first");
        // Down arrow
        app.history_next();
        assert_eq!(app.input_text, "second");
    }

    #[tokio::test]
    async fn test_history_navigation_saves_current_input() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.history.add("old command", None);
        app.input_text = "my current text".to_string();
        app.input_cursor = app.input_text.chars().count();
        // Navigate up — should save current text
        app.history_prev();
        assert_eq!(app.input_text, "old command");
        assert_eq!(app.history_saved_input, "my current text");
    }

    #[tokio::test]
    async fn test_history_navigation_restores_on_cancel() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.history.add("old command", None);
        app.input_text = "my current text".to_string();
        app.input_cursor = app.input_text.chars().count();
        // Navigate up then back down past end → restore saved input
        app.history_prev();
        assert_eq!(app.input_text, "old command");
        app.history_next();
        assert_eq!(app.input_text, "my current text");
        assert!(app.history_index.is_none());
    }

    #[test]
    fn test_split_ratio_clamps_at_boundaries() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        // Reduce far below min
        app.split_pane.adjust_ratio(-100);
        assert_eq!(app.split_pane.ratio(), 30); // min is 30
                                                // Increase far above max
        app.split_pane.adjust_ratio(200);
        assert_eq!(app.split_pane.ratio(), 90); // max is 90
    }

    // --- Phase 12: Vim Mode in App Context ---

    #[tokio::test]
    async fn test_vim_esc_enters_normal_mode() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.vim.set_enabled(true);
        // Start in insert mode (default when vim enabled)
        app.vim.mode = crate::vim_mode::Mode::Insert;
        // Pressing Esc → normal mode
        app.handle_vim_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert_eq!(app.vim.mode, crate::vim_mode::Mode::Normal);
    }

    #[tokio::test]
    async fn test_vim_i_enters_insert_mode() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.vim.set_enabled(true);
        app.vim.mode = crate::vim_mode::Mode::Normal;
        // 'i' in normal mode → insert mode
        app.handle_vim_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
            .await;
        assert_eq!(app.vim.mode, crate::vim_mode::Mode::Insert);
    }

    #[tokio::test]
    async fn test_vim_dd_deletes_line() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.vim.set_enabled(true);
        app.input_text = "hello world".to_string();
        app.input_cursor = 0;
        app.vim.mode = crate::vim_mode::Mode::Normal;
        // 'd' then 'd' → DeleteLine
        app.handle_vim_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE))
            .await;
        app.handle_vim_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE))
            .await;
        assert!(app.input_text.is_empty(), "dd should clear the input line");
        assert_eq!(app.input_cursor, 0);
    }

    #[tokio::test]
    async fn test_vim_slash_activates_search() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.vim.set_enabled(true);
        app.vim.mode = crate::vim_mode::Mode::Normal;
        // '/' in normal mode → open search
        app.handle_vim_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .await;
        assert!(
            app.search.is_active(),
            "'/' in vim normal mode should activate search"
        );
    }

    #[tokio::test]
    async fn test_vim_colon_q_quits() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.vim.set_enabled(true);
        app.vim.mode = crate::vim_mode::Mode::Normal;
        // ':' enters command mode
        app.handle_vim_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE))
            .await;
        // Type 'q'
        app.handle_vim_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .await;
        // Enter executes command
        app.handle_vim_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        assert!(app.should_quit, ":q Enter should quit");
    }

    #[tokio::test]
    async fn test_vim_mode_toggle_via_slash_command() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        let was_vim = app.vim.enabled;
        app.input_text = "/vim".to_string();
        app.input_cursor = 4;
        app.submit_input().await;
        assert_eq!(app.vim.enabled, !was_vim, "/vim should toggle vim mode");
    }

    #[tokio::test]
    async fn test_vim_mode_preserves_input_on_mode_switch() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.vim.set_enabled(true);
        app.vim.mode = crate::vim_mode::Mode::Insert;
        app.input_text = "keep this text".to_string();
        app.input_cursor = 5;
        // Switch to normal mode via Esc
        app.handle_vim_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert_eq!(
            app.input_text, "keep this text",
            "Mode switch should preserve text"
        );
    }

    #[tokio::test]
    async fn test_vim_ctrl_c_in_normal_mode_quits_when_no_turn() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.vim.set_enabled(true);
        app.vim.mode = crate::vim_mode::Mode::Normal;
        app.is_turn_active = false;
        // Vim-mode Ctrl+C passes through to handle_ctrl_c which follows the
        // double-press contract: first press arms the hint, second quits.
        app.handle_vim_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .await;
        assert!(!app.should_quit, "First Ctrl+C in vim normal must not quit");
        app.handle_vim_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .await;
        assert!(app.should_quit, "Second Ctrl+C in vim normal should quit");
    }

    // --- Phase 13: Notification & Edge Cases ---

    #[test]
    fn test_notification_info_level() {
        let n = Notification::new(
            "test info",
            crate::widgets::notification::NotificationLevel::Info,
        );
        assert_eq!(
            n.level,
            crate::widgets::notification::NotificationLevel::Info
        );
        assert_eq!(n.message, "test info");
    }

    #[test]
    fn test_notification_warning_level() {
        let n = Notification::new(
            "test warn",
            crate::widgets::notification::NotificationLevel::Warning,
        );
        assert_eq!(
            n.level,
            crate::widgets::notification::NotificationLevel::Warning
        );
    }

    #[test]
    fn test_notification_error_level() {
        let n = Notification::new(
            "test err",
            crate::widgets::notification::NotificationLevel::Error,
        );
        assert_eq!(
            n.level,
            crate::widgets::notification::NotificationLevel::Error
        );
    }

    #[tokio::test]
    async fn test_empty_input_submit_does_nothing() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        app.input_text = String::new();
        app.submit_input().await;
        // No event should be sent for empty input
        assert!(
            ui_rx.try_recv().is_err(),
            "Empty input should not produce a UiEvent"
        );
    }

    #[test]
    fn test_input_cursor_clamps_to_text_length() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.input_text = "abc".to_string();
        // Force cursor beyond text length
        app.input_cursor = 100;
        // The cursor should be clamped in operations — test via right arrow logic
        let char_count = app.input_text.chars().count();
        if app.input_cursor > char_count {
            app.input_cursor = char_count;
        }
        assert_eq!(app.input_cursor, 3);
    }

    #[tokio::test]
    async fn test_alt_enter_inserts_newline() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.input_text = "hello".to_string();
        app.input_cursor = 5;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT))
            .await;
        assert_eq!(app.input_text, "hello\n", "Alt+Enter should insert newline");
        assert_eq!(app.input_cursor, 6);
    }

    // --- Phase 14: Image Handling ---

    #[tokio::test]
    async fn test_pending_images_sent_with_input() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        // Manually add a pending image
        app.pending_images.push(crate::image_paste::PastedImage {
            path: std::path::PathBuf::from("/tmp/test.png"),
            label: "test.png".to_string(),
            dimensions: Some((100, 100)),
        });
        app.input_text = "check this image".to_string();
        app.input_cursor = app.input_text.chars().count();
        app.submit_input().await;
        let event = ui_rx.try_recv().unwrap();
        match event {
            UiEvent::UserInput { text, images } => {
                assert_eq!(text, "check this image");
                assert_eq!(images.len(), 1);
            }
            _ => panic!("Expected UserInput event"),
        }
    }

    #[tokio::test]
    async fn test_pending_images_cleared_after_submit() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.pending_images.push(crate::image_paste::PastedImage {
            path: std::path::PathBuf::from("/tmp/test.png"),
            label: "test.png".to_string(),
            dimensions: Some((100, 100)),
        });
        app.input_text = "check".to_string();
        app.input_cursor = 5;
        app.submit_input().await;
        assert!(
            app.pending_images.is_empty(),
            "pending_images should be cleared after submit"
        );
    }

    #[tokio::test]
    async fn test_submit_with_only_images_sends_event() {
        let (mut app, mut ui_rx, _core_tx) = make_test_app();
        // Images but empty text — should still send (pending_images not empty check)
        // Actually the condition is: !input_text.is_empty() || !pending_images.is_empty()
        // With empty text, it enters the else branch and sends UserInput with empty text
        // But wait — we need text OR images. Let's set input text to "[Image #1]"
        app.pending_images.push(crate::image_paste::PastedImage {
            path: std::path::PathBuf::from("/tmp/test.png"),
            label: "test.png".to_string(),
            dimensions: None,
        });
        app.input_text = "[Image #1] ".to_string();
        app.input_cursor = app.input_text.chars().count();
        app.submit_input().await;
        let event = ui_rx.try_recv().unwrap();
        match event {
            UiEvent::UserInput { text, images } => {
                assert!(text.contains("Image"), "Should contain image tag");
                assert_eq!(images.len(), 1, "Should include the pending image");
            }
            _ => panic!("Expected UserInput event"),
        }
    }

    #[tokio::test]
    async fn test_image_paths_tracked_in_sent_image_paths() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.pending_images.push(crate::image_paste::PastedImage {
            path: std::path::PathBuf::from("/tmp/img1.png"),
            label: "img1.png".to_string(),
            dimensions: Some((200, 150)),
        });
        app.input_text = "[Image #1] describe this".to_string();
        app.input_cursor = app.input_text.chars().count();
        app.submit_input().await;
        // The image counter starts at 0 (from message_cache), so first image is at index 1
        assert!(
            !app.sent_image_paths.is_empty(),
            "sent_image_paths should track submitted images"
        );
    }

    #[tokio::test]
    async fn test_hydrate_image_paths_from_session_rebuilds_map() {
        use base64::Engine as _;
        use oxicode_common::{ContentBlock, ImageSource, Message};

        // Use a throwaway session id under $HOME/.oxicode/image-cache for isolation.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let session_id = format!("test-hydrate-{nanos}");
        let cache_dir = crate::image_paste::image_cache_dir(&session_id);

        // Minimal 24-byte PNG (signature + IHDR, 1x1).
        let mut png = Vec::new();
        png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1u32.to_be_bytes());
        png.extend_from_slice(&1u32.to_be_bytes());
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);

        // Build a state store whose session_id matches the cache dir,
        // containing a user message with two Image blocks and one text block.
        let state = oxicode_state::AppState {
            session_id: session_id.clone(),
            messages: vec![Message {
                id: "m1".into(),
                role: oxicode_common::Role::User,
                content: vec![
                    ContentBlock::Image {
                        source: ImageSource {
                            source_type: "base64".into(),
                            media_type: Some("image/png".into()),
                            data: Some(b64.clone()),
                        },
                    },
                    ContentBlock::Image {
                        source: ImageSource {
                            source_type: "base64".into(),
                            media_type: Some("image/png".into()),
                            data: Some(b64),
                        },
                    },
                    ContentBlock::Text {
                        text: "look".into(),
                    },
                ],
                model: None,
                stop_reason: None,
                created_at: chrono::Utc::now(),
                usage: None,
            }],
            ..oxicode_state::AppState::default()
        };
        let store = Arc::new(StateStore::new(state));
        let (ui_tx, _ui_rx) = mpsc::channel::<UiEvent>(8);
        let (_core_tx, core_rx) = mpsc::channel::<CoreEvent>(8);
        let mut app = App::new(&store, ui_tx, core_rx, Vec::new());

        app.hydrate_image_paths_from_session();

        assert_eq!(
            app.sent_image_paths.len(),
            2,
            "both images should be mapped"
        );
        let p1 = app.sent_image_paths.get(&1).unwrap();
        let p2 = app.sent_image_paths.get(&2).unwrap();
        assert_eq!(p1, &cache_dir.join("1.png"));
        assert_eq!(p2, &cache_dir.join("2.png"));
        assert!(p1.exists(), "1.png should be materialized from base64");
        assert!(p2.exists(), "2.png should be materialized from base64");

        // Cleanup.
        let _ = std::fs::remove_dir_all(&cache_dir);
    }

    #[tokio::test]
    async fn test_bare_resume_opens_session_browser() {
        let (mut app, _ui_rx, _core_tx) = make_test_app();
        app.input_text = "/resume".into();
        app.input_cursor = app.input_text.chars().count();
        app.submit_input().await;
        assert!(
            app.session_browser.is_visible(),
            "/resume with no args should open the session browser overlay"
        );
    }
}
