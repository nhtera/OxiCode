//! Integration tests for Vim mode state machine and Keybinding registry.
//!
//! Covers: mode transitions, operators + motions, text objects, count prefixes,
//! visual mode, command mode, word navigation functions, keybinding lookup/
//! override/unbind/parse, and TOML file loading.
//!
//! No API key needed — pure logic tests.
//! Run with: `cargo test -p oxicode-tui --test tui_vim_and_keybinding_integration_tests`

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::Write;

use oxicode_tui::keybindings::{Action, KeyCombo, KeybindingRegistry};
use oxicode_tui::vim_mode::{next_word_pos, prev_word_pos, word_end_pos, Mode, VimAction, VimState};

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

// ═══════════════════════════════════════════════════════════════════
// A. Vim Mode — Full Operator + Motion Chains
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_dd_yy_cc_operator_doubled() {
    let mut vim = VimState::new(true);

    // dd → DeleteLine
    vim.handle_key(key(KeyCode::Char('d')), 10);
    assert_eq!(vim.handle_key(key(KeyCode::Char('d')), 10), VimAction::DeleteLine);

    // yy → YankLine
    vim.handle_key(key(KeyCode::Char('y')), 10);
    assert_eq!(vim.handle_key(key(KeyCode::Char('y')), 10), VimAction::YankLine);

    // cc → DeleteLine + switch to Insert
    vim.handle_key(key(KeyCode::Char('c')), 10);
    let action = vim.handle_key(key(KeyCode::Char('c')), 10);
    assert_eq!(action, VimAction::DeleteLine);
    assert_eq!(vim.mode, Mode::Insert);
}

#[test]
fn test_dw_db_d_dollar_motions() {
    let mut vim = VimState::new(true);

    // dw → DeleteWordForward
    vim.handle_key(key(KeyCode::Char('d')), 20);
    assert_eq!(vim.handle_key(key(KeyCode::Char('w')), 20), VimAction::DeleteWordForward);

    // db → DeleteWordBackward
    vim.handle_key(key(KeyCode::Char('d')), 20);
    assert_eq!(vim.handle_key(key(KeyCode::Char('b')), 20), VimAction::DeleteWordBackward);

    // d$ → DeleteToEnd
    vim.handle_key(key(KeyCode::Char('d')), 20);
    assert_eq!(vim.handle_key(key(KeyCode::Char('$')), 20), VimAction::DeleteToEnd);
}

#[test]
fn test_cw_c_dollar_change_motions() {
    let mut vim = VimState::new(true);

    // cw → DeleteWordForward + Insert
    vim.handle_key(key(KeyCode::Char('c')), 20);
    assert_eq!(vim.handle_key(key(KeyCode::Char('w')), 20), VimAction::DeleteWordForward);
    assert_eq!(vim.mode, Mode::Insert);

    // Reset to normal.
    vim.handle_key(key(KeyCode::Esc), 20);

    // c$ → ChangeToEnd + Insert
    vim.handle_key(key(KeyCode::Char('c')), 20);
    assert_eq!(vim.handle_key(key(KeyCode::Char('$')), 20), VimAction::ChangeToEnd);
    assert_eq!(vim.mode, Mode::Insert);
}

// ═══════════════════════════════════════════════════════════════════
// B. Vim Mode — Text Objects (di", ci(, ya', etc.)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_diw_delete_inner_word() {
    let mut vim = VimState::new(true);
    // d → i → w = DeleteTextObject('i', 'w')
    vim.handle_key(key(KeyCode::Char('d')), 10);
    vim.handle_key(key(KeyCode::Char('i')), 10);
    let action = vim.handle_key(key(KeyCode::Char('w')), 10);
    assert_eq!(action, VimAction::DeleteTextObject('i', 'w'));
}

#[test]
fn test_ci_double_quote_change_inner_quote() {
    let mut vim = VimState::new(true);
    // c → i → " = ChangeTextObject('i', '"') + Insert mode
    vim.handle_key(key(KeyCode::Char('c')), 10);
    vim.handle_key(key(KeyCode::Char('i')), 10);
    let action = vim.handle_key(key(KeyCode::Char('"')), 10);
    assert_eq!(action, VimAction::ChangeTextObject('i', '"'));
    assert_eq!(vim.mode, Mode::Insert);
}

#[test]
fn test_ya_paren_yank_around_parens() {
    let mut vim = VimState::new(true);
    // y → a → ( = YankTextObject('a', '(')
    vim.handle_key(key(KeyCode::Char('y')), 10);
    vim.handle_key(key(KeyCode::Char('a')), 10);
    let action = vim.handle_key(key(KeyCode::Char('(')), 10);
    assert_eq!(action, VimAction::YankTextObject('a', '('));
}

#[test]
fn test_text_object_invalid_target_noop() {
    let mut vim = VimState::new(true);
    // d → i → z (not a recognized text object target)
    vim.handle_key(key(KeyCode::Char('d')), 10);
    vim.handle_key(key(KeyCode::Char('i')), 10);
    let action = vim.handle_key(key(KeyCode::Char('z')), 10);
    assert_eq!(action, VimAction::Noop);
}

#[test]
fn test_all_text_object_targets_recognized() {
    let targets = ['w', '"', '\'', '(', ')', '{', '}', '[', ']', '<', '>', '`'];
    for target in targets {
        let mut vim = VimState::new(true);
        vim.handle_key(key(KeyCode::Char('d')), 10);
        vim.handle_key(key(KeyCode::Char('i')), 10);
        let action = vim.handle_key(key(KeyCode::Char(target)), 10);
        assert_eq!(
            action,
            VimAction::DeleteTextObject('i', target),
            "target '{target}' should be recognized"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// C. Vim Mode — Count Prefix
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_count_prefix_with_motion() {
    let mut vim = VimState::new(true);

    // 5l → MoveCursorBy(5)
    vim.handle_key(key(KeyCode::Char('5')), 20);
    assert_eq!(vim.handle_key(key(KeyCode::Char('l')), 20), VimAction::MoveCursorBy(5));

    // 3h → MoveCursorBy(-3)
    vim.handle_key(key(KeyCode::Char('3')), 20);
    assert_eq!(vim.handle_key(key(KeyCode::Char('h')), 20), VimAction::MoveCursorBy(-3));
}

#[test]
fn test_count_prefix_multi_digit() {
    let mut vim = VimState::new(true);
    // 12l → MoveCursorBy(12)
    vim.handle_key(key(KeyCode::Char('1')), 50);
    vim.handle_key(key(KeyCode::Char('2')), 50);
    assert_eq!(vim.handle_key(key(KeyCode::Char('l')), 50), VimAction::MoveCursorBy(12));
}

#[test]
fn test_count_prefix_with_word_motion() {
    let mut vim = VimState::new(true);
    // 3w → MoveWordForward(3)
    vim.handle_key(key(KeyCode::Char('3')), 50);
    assert_eq!(vim.handle_key(key(KeyCode::Char('w')), 50), VimAction::MoveWordForward(3));

    // 2b → MoveWordBackward(2)
    vim.handle_key(key(KeyCode::Char('2')), 50);
    assert_eq!(vim.handle_key(key(KeyCode::Char('b')), 50), VimAction::MoveWordBackward(2));
}

#[test]
fn test_zero_not_count_prefix_goes_to_line_start() {
    let mut vim = VimState::new(true);
    // '0' without prior digits → MoveToLineStart
    assert_eq!(vim.handle_key(key(KeyCode::Char('0')), 10), VimAction::MoveToLineStart);
}

#[test]
fn test_esc_clears_pending_count() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char('3')), 10);
    vim.handle_key(key(KeyCode::Esc), 10);
    // After Esc, next 'l' should use default count=1
    assert_eq!(vim.handle_key(key(KeyCode::Char('l')), 10), VimAction::MoveCursorBy(1));
}

// ═══════════════════════════════════════════════════════════════════
// D. Vim Mode — Visual Mode Interactions
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_visual_mode_motions() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char('v')), 20);
    assert_eq!(vim.mode, Mode::Visual);

    // Motions in visual mode.
    assert_eq!(vim.handle_key(key(KeyCode::Char('l')), 20), VimAction::MoveCursorBy(1));
    assert_eq!(vim.handle_key(key(KeyCode::Char('h')), 20), VimAction::MoveCursorBy(-1));
    assert_eq!(vim.handle_key(key(KeyCode::Char('w')), 20), VimAction::MoveWordForward(1));
    assert_eq!(vim.handle_key(key(KeyCode::Char('b')), 20), VimAction::MoveWordBackward(1));
    assert_eq!(vim.handle_key(key(KeyCode::Char('0')), 20), VimAction::MoveToLineStart);
    assert_eq!(vim.handle_key(key(KeyCode::Char('$')), 20), VimAction::MoveToLineEnd);
}

#[test]
fn test_visual_mode_delete_yields_range() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char('v')), 20);
    let action = vim.handle_key(key(KeyCode::Char('d')), 20);
    // Placeholder range — app.rs resolves actual anchor+cursor.
    assert_eq!(action, VimAction::DeleteRange(0, 0));
    assert_eq!(vim.mode, Mode::Normal, "visual d returns to normal");
}

#[test]
fn test_visual_mode_change_yields_range() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char('v')), 20);
    let action = vim.handle_key(key(KeyCode::Char('c')), 20);
    assert_eq!(action, VimAction::ChangeRange(0, 0));
    assert_eq!(vim.mode, Mode::Insert, "visual c enters insert");
}

#[test]
fn test_visual_mode_yank_yields_range() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char('v')), 20);
    let action = vim.handle_key(key(KeyCode::Char('y')), 20);
    assert_eq!(action, VimAction::YankRange(0, 0));
    assert_eq!(vim.mode, Mode::Normal, "visual y returns to normal");
}

#[test]
fn test_visual_line_mode() {
    let mut vim = VimState::new(true);
    let action = vim.handle_key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT), 20);
    assert_eq!(action, VimAction::EnterVisualLine);
    assert_eq!(vim.mode, Mode::VisualLine);

    // Esc returns to Normal.
    vim.handle_key(key(KeyCode::Esc), 20);
    assert_eq!(vim.mode, Mode::Normal);
}

// ═══════════════════════════════════════════════════════════════════
// E. Vim Mode — Command Mode
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_command_mode_wq_quits() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char(':')), 10);
    assert_eq!(vim.mode, Mode::Command);

    vim.handle_key(key(KeyCode::Char('w')), 10);
    vim.handle_key(key(KeyCode::Char('q')), 10);
    let action = vim.handle_key(key(KeyCode::Enter), 10);
    assert_eq!(action, VimAction::Quit);
    assert_eq!(vim.mode, Mode::Normal);
}

#[test]
fn test_command_mode_custom_command() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char(':')), 10);
    for c in "set nu".chars() {
        vim.handle_key(key(KeyCode::Char(c)), 10);
    }
    let action = vim.handle_key(key(KeyCode::Enter), 10);
    assert_eq!(action, VimAction::ExecuteCommand("set nu".to_string()));
}

#[test]
fn test_command_mode_esc_cancels() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char(':')), 10);
    vim.handle_key(key(KeyCode::Char('q')), 10);
    vim.handle_key(key(KeyCode::Esc), 10);
    assert_eq!(vim.mode, Mode::Normal);
    assert!(vim.command_buffer().is_empty());
}

#[test]
fn test_command_mode_backspace_empty_exits() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char(':')), 10);
    assert_eq!(vim.mode, Mode::Command);

    // Backspace on empty command buffer exits command mode.
    vim.handle_key(key(KeyCode::Backspace), 10);
    assert_eq!(vim.mode, Mode::Normal);
}

#[test]
fn test_command_mode_backspace_non_empty_deletes() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char(':')), 10);
    vim.handle_key(key(KeyCode::Char('q')), 10);
    assert_eq!(vim.command_buffer(), "q");

    vim.handle_key(key(KeyCode::Backspace), 10);
    assert!(vim.command_buffer().is_empty());
    assert_eq!(vim.mode, Mode::Command, "still in command mode");
}

// ═══════════════════════════════════════════════════════════════════
// F. Vim Mode — Insert Mode Interactions
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_insert_mode_ctrl_w_deletes_word() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char('i')), 10);
    assert_eq!(vim.mode, Mode::Insert);
    assert_eq!(vim.handle_key(ctrl('w'), 10), VimAction::DeleteWordBackward);
}

#[test]
fn test_insert_mode_ctrl_u_deletes_line() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char('i')), 10);
    assert_eq!(vim.handle_key(ctrl('u'), 10), VimAction::DeleteLine);
}

#[test]
fn test_insert_mode_arrows_and_home_end() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char('i')), 10);

    assert_eq!(vim.handle_key(key(KeyCode::Left), 10), VimAction::MoveCursorBy(-1));
    assert_eq!(vim.handle_key(key(KeyCode::Right), 10), VimAction::MoveCursorBy(1));
    assert_eq!(vim.handle_key(key(KeyCode::Home), 10), VimAction::MoveToLineStart);
    assert_eq!(vim.handle_key(key(KeyCode::End), 10), VimAction::MoveToLineEnd);
}

#[test]
fn test_insert_mode_enter_submits() {
    let mut vim = VimState::new(true);
    vim.handle_key(key(KeyCode::Char('i')), 5);
    assert_eq!(vim.handle_key(key(KeyCode::Enter), 5), VimAction::Submit);
}

// ═══════════════════════════════════════════════════════════════════
// G. Vim Mode — Mode Entry Variants
// ═══════════════════════════════════════════════════════════════════

#[test]
#[allow(non_snake_case)]
fn test_mode_entries_a_A_I_o() {
    let mut vim = VimState::new(true);

    // a → AppendAfterCursor + Insert
    let action = vim.handle_key(key(KeyCode::Char('a')), 10);
    assert_eq!(action, VimAction::AppendAfterCursor);
    assert_eq!(vim.mode, Mode::Insert);
    vim.handle_key(key(KeyCode::Esc), 10);

    // A → AppendAtEnd + Insert
    let action = vim.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT), 10);
    assert_eq!(action, VimAction::AppendAtEnd);
    assert_eq!(vim.mode, Mode::Insert);
    vim.handle_key(key(KeyCode::Esc), 10);

    // I → InsertAtLineStart + Insert
    let action = vim.handle_key(KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT), 10);
    assert_eq!(action, VimAction::InsertAtLineStart);
    assert_eq!(vim.mode, Mode::Insert);
    vim.handle_key(key(KeyCode::Esc), 10);

    // o → OpenLineBelow + Insert
    let action = vim.handle_key(key(KeyCode::Char('o')), 10);
    assert_eq!(action, VimAction::OpenLineBelow);
    assert_eq!(vim.mode, Mode::Insert);
}

#[test]
#[allow(non_snake_case)]
fn test_D_deletes_to_end() {
    let mut vim = VimState::new(true);
    let action = vim.handle_key(KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT), 10);
    assert_eq!(action, VimAction::DeleteToEnd);
}

#[test]
#[allow(non_snake_case)]
fn test_C_changes_to_end_enters_insert() {
    let mut vim = VimState::new(true);
    let action = vim.handle_key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT), 10);
    assert_eq!(action, VimAction::ChangeToEnd);
    assert_eq!(vim.mode, Mode::Insert);
}

#[test]
#[allow(non_snake_case)]
fn test_G_moves_to_end() {
    let mut vim = VimState::new(true);
    let action = vim.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT), 10);
    assert_eq!(action, VimAction::MoveToEnd);
}

// ═══════════════════════════════════════════════════════════════════
// H. Vim Mode — Word Navigation Functions
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_next_word_pos_multiple_words() {
    let text = "hello world foo bar baz";
    assert_eq!(next_word_pos(text, 0), 6);    // "hello " → "world"
    assert_eq!(next_word_pos(text, 6), 12);   // "world " → "foo"
    assert_eq!(next_word_pos(text, 12), 16);  // "foo " → "bar"
    assert_eq!(next_word_pos(text, 16), 20);  // "bar " → "baz"
    assert_eq!(next_word_pos(text, 20), 23);  // "baz" → end
}

#[test]
fn test_prev_word_pos_multiple_words() {
    let text = "hello world foo bar baz";
    assert_eq!(prev_word_pos(text, 23), 20);  // end → "baz"
    assert_eq!(prev_word_pos(text, 20), 16);  // "baz" → "bar"
    assert_eq!(prev_word_pos(text, 16), 12);  // "bar" → "foo"
    assert_eq!(prev_word_pos(text, 12), 6);   // "foo" → "world"
    assert_eq!(prev_word_pos(text, 6), 0);    // "world" → "hello"
    assert_eq!(prev_word_pos(text, 0), 0);    // already at start
}

#[test]
fn test_word_end_pos_multiple_words() {
    let text = "hello world foo";
    assert_eq!(word_end_pos(text, 0), 4);     // "hello" → end of "hello" (index 4)
    assert_eq!(word_end_pos(text, 6), 10);    // "world" → end of "world" (index 10)
}

#[test]
fn test_word_navigation_single_word() {
    let text = "hello";
    assert_eq!(next_word_pos(text, 0), 5);   // past end
    assert_eq!(prev_word_pos(text, 5), 0);
    assert_eq!(word_end_pos(text, 0), 4);
}

#[test]
fn test_word_navigation_empty_string() {
    assert_eq!(next_word_pos("", 0), 0);
    assert_eq!(prev_word_pos("", 0), 0);
}

#[test]
fn test_word_navigation_multiple_spaces() {
    let text = "hello   world";
    assert_eq!(next_word_pos(text, 0), 8);   // skip multi-space gap
    assert_eq!(prev_word_pos(text, 8), 0);   // back over multi-space gap
}

// ═══════════════════════════════════════════════════════════════════
// I. Vim Mode — Yank Register
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_yank_register_stores_and_retrieves() {
    let mut vim = VimState::new(true);
    assert!(vim.yanked().is_empty());

    vim.yank("hello world");
    assert_eq!(vim.yanked(), "hello world");

    // Overwrite.
    vim.yank("new content");
    assert_eq!(vim.yanked(), "new content");
}

#[test]
fn test_visual_anchor_set_and_get() {
    let mut vim = VimState::new(true);
    vim.set_visual_anchor(42);
    assert_eq!(vim.visual_anchor(), 42);
}

// ═══════════════════════════════════════════════════════════════════
// J. Vim Mode — Full Workflow Scenario
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_vim_workflow_edit_delete_paste() {
    let mut vim = VimState::new(true);

    // Normal → i → type "hello" → Esc → dd → p workflow.
    // Step 1: Enter insert.
    assert_eq!(vim.handle_key(key(KeyCode::Char('i')), 0), VimAction::SwitchToInsert);
    assert_eq!(vim.mode, Mode::Insert);

    // Step 2: Type characters.
    for c in "hello".chars() {
        assert_eq!(vim.handle_key(key(KeyCode::Char(c)), 5), VimAction::InsertChar(c));
    }

    // Step 3: Esc to Normal.
    vim.handle_key(key(KeyCode::Esc), 5);
    assert_eq!(vim.mode, Mode::Normal);

    // Step 4: dd to delete line.
    vim.handle_key(key(KeyCode::Char('d')), 5);
    assert_eq!(vim.handle_key(key(KeyCode::Char('d')), 5), VimAction::DeleteLine);

    // Step 5: p to paste.
    assert_eq!(vim.handle_key(key(KeyCode::Char('p')), 0), VimAction::Paste);
}

#[test]
fn test_vim_workflow_search_and_command() {
    let mut vim = VimState::new(true);

    // '/' → EnterSearch
    assert_eq!(vim.handle_key(key(KeyCode::Char('/')), 10), VimAction::EnterSearch);

    // ':wq' → Quit
    vim.handle_key(key(KeyCode::Char(':')), 10);
    assert_eq!(vim.mode, Mode::Command);
    vim.handle_key(key(KeyCode::Char('w')), 10);
    vim.handle_key(key(KeyCode::Char('q')), 10);
    assert_eq!(vim.handle_key(key(KeyCode::Enter), 10), VimAction::Quit);
}

// ═══════════════════════════════════════════════════════════════════
// K. Keybinding Registry — Comprehensive Lookup
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_all_default_bindings_present() {
    let reg = KeybindingRegistry::with_defaults();

    // Verify all expected default bindings exist.
    let expected: Vec<(KeyModifiers, KeyCode, Action)> = vec![
        (KeyModifiers::CONTROL, KeyCode::Char('c'), Action::Quit),
        (KeyModifiers::NONE, KeyCode::Enter, Action::Submit),
        (KeyModifiers::NONE, KeyCode::Tab, Action::TogglePanel),
        (KeyModifiers::NONE, KeyCode::PageUp, Action::PageUp),
        (KeyModifiers::NONE, KeyCode::PageDown, Action::PageDown),
        (KeyModifiers::CONTROL, KeyCode::Char('k'), Action::ClearLine),
        (KeyModifiers::CONTROL, KeyCode::Char('w'), Action::DeleteWordBackward),
        (KeyModifiers::CONTROL, KeyCode::Char('u'), Action::DeleteToLineStart),
        (KeyModifiers::NONE, KeyCode::Home, Action::CursorHome),
        (KeyModifiers::NONE, KeyCode::End, Action::CursorEnd),
        (KeyModifiers::CONTROL, KeyCode::Char('a'), Action::CursorHome),
        (KeyModifiers::CONTROL, KeyCode::Char('e'), Action::CursorEnd),
        (KeyModifiers::ALT, KeyCode::Left, Action::CursorWordLeft),
        (KeyModifiers::ALT, KeyCode::Right, Action::CursorWordRight),
        (KeyModifiers::SHIFT, KeyCode::Enter, Action::InsertNewline),
        (KeyModifiers::ALT, KeyCode::Enter, Action::InsertNewline),
        (KeyModifiers::CONTROL, KeyCode::Char('r'), Action::HistorySearch),
        (KeyModifiers::CONTROL, KeyCode::Char('t'), Action::ToggleThinking),
        (KeyModifiers::CONTROL, KeyCode::Char('f'), Action::OpenSearch),
        (KeyModifiers::NONE, KeyCode::F(2), Action::OpenModelPicker),
        (KeyModifiers::NONE, KeyCode::F(3), Action::OpenSessionBrowser),
    ];

    for (modifiers, code, expected_action) in &expected {
        let ke = KeyEvent::new(*code, *modifiers);
        assert_eq!(
            reg.lookup(&ke),
            Some(expected_action),
            "Missing default binding for {:?}+{:?}",
            modifiers,
            code,
        );
    }
}

#[test]
fn test_keybinding_override_replaces_existing() {
    let mut reg = KeybindingRegistry::with_defaults();

    // Override Ctrl+C from Quit to Submit.
    reg.bind(
        KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c')),
        Action::Submit,
    );
    let ke = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(reg.lookup(&ke), Some(&Action::Submit));
}

#[test]
fn test_keybinding_unbind_and_rebind() {
    let mut reg = KeybindingRegistry::with_defaults();

    // Unbind Ctrl+C.
    let combo = KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c'));
    reg.unbind(&combo);
    let ke = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(reg.lookup(&ke), None);

    // Rebind to different action.
    reg.bind(combo, Action::ScrollUp);
    assert_eq!(reg.lookup(&ke), Some(&Action::ScrollUp));
}

#[test]
fn test_keybinding_unknown_key_returns_none() {
    let reg = KeybindingRegistry::with_defaults();
    let ke = KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE);
    assert_eq!(reg.lookup(&ke), None);
}

// ═══════════════════════════════════════════════════════════════════
// L. Keybinding — Parse Key Combo
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_parse_key_combo_various_formats() {
    // Test via bind + lookup roundtrip (parse_key_combo is private, but tested via TOML loading).
    let mut reg = KeybindingRegistry::with_defaults();

    // Load from temporary TOML.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keybindings.toml");
    let mut file = std::fs::File::create(&path).unwrap();
    writeln!(file, r#""Ctrl+S" = "submit""#).unwrap();
    writeln!(file, r#""Alt+Enter" = "insert_newline""#).unwrap();
    writeln!(file, r#""F5" = "toggle_panel""#).unwrap();
    drop(file);

    reg.load_from_file(&path);

    // Verify parsed bindings.
    let ke_ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(reg.lookup(&ke_ctrl_s), Some(&Action::Submit));

    let ke_alt_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
    assert_eq!(reg.lookup(&ke_alt_enter), Some(&Action::InsertNewline));

    let ke_f5 = KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE);
    assert_eq!(reg.lookup(&ke_f5), Some(&Action::TogglePanel));
}

#[test]
fn test_toml_loading_invalid_entries_skipped() {
    let mut reg = KeybindingRegistry::with_defaults();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keybindings.toml");
    let mut file = std::fs::File::create(&path).unwrap();
    writeln!(file, r#""InvalidKey" = "unknown_action""#).unwrap();
    writeln!(file, r#""Ctrl+S" = "submit""#).unwrap(); // Valid entry mixed in.
    drop(file);

    reg.load_from_file(&path);

    // Valid entry should be loaded.
    let ke = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(reg.lookup(&ke), Some(&Action::Submit));
}

// ═══════════════════════════════════════════════════════════════════
// M. Keybinding — Key Combo Label
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_key_combo_label_various() {
    assert_eq!(
        KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('k')).label(),
        "Ctrl+K"
    );
    assert_eq!(
        KeyCombo::new(KeyModifiers::ALT, KeyCode::Enter).label(),
        "Alt+Enter"
    );
    assert_eq!(
        KeyCombo::new(KeyModifiers::SHIFT, KeyCode::Tab).label(),
        "Shift+Tab"
    );
    assert_eq!(
        KeyCombo::new(KeyModifiers::NONE, KeyCode::F(2)).label(),
        "F2"
    );
    assert_eq!(
        KeyCombo::new(KeyModifiers::NONE, KeyCode::PageUp).label(),
        "PageUp"
    );
    assert_eq!(
        KeyCombo::new(KeyModifiers::CONTROL | KeyModifiers::SHIFT, KeyCode::Char('a')).label(),
        "Ctrl+Shift+A"
    );
}

#[test]
fn test_key_combo_matches_key_event() {
    let combo = KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('f'));
    assert!(combo.matches(&KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)));
    assert!(!combo.matches(&KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE)));
    assert!(!combo.matches(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL)));
}

// ═══════════════════════════════════════════════════════════════════
// N. Keybinding — List Bindings
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_list_bindings_sorted() {
    let reg = KeybindingRegistry::with_defaults();
    let list = reg.list_bindings();

    // Should have all default bindings.
    assert!(list.len() >= 20, "should have >= 20 default bindings, got: {}", list.len());

    // Labels should be sorted.
    let labels: Vec<&str> = list.iter().map(|(label, _)| label.as_str()).collect();
    let mut sorted_labels = labels.clone();
    sorted_labels.sort();
    assert_eq!(labels, sorted_labels, "bindings should be sorted by label");
}
