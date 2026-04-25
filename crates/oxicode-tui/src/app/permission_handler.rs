/// Permission dialog key handler.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::widgets::Notification;

impl super::App {
    /// Handle key events when the permission dialog is active.
    #[allow(clippy::too_many_lines)]
    pub(super) async fn handle_permission_key(&mut self, key: KeyEvent) {
        let Some(ref mut perm) = self.pending_permission else {
            return;
        };
        let max_idx = perm.kind.option_count().saturating_sub(1);
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) => {
                perm.selected = perm.selected.saturating_sub(1);
            }
            (_, KeyCode::Down) => {
                perm.selected = (perm.selected + 1).min(max_idx);
            }
            (_, KeyCode::Enter) => {
                // Map selected index to response based on dialog kind.
                // FileRead (3 opts): 0=AllowOnce, 1=AlwaysAllow, 2=Deny
                // Bash    (5 opts): 0=AllowOnce, 1=AlwaysAllow, 2=PrefixAllow, 3=Deny, 4=AlwaysDeny
                // Others  (4 opts): 0=AllowOnce, 1=AlwaysAllow, 2=Deny, 3=AlwaysDeny
                let is_bash =
                    matches!(perm.kind, crate::widgets::PermissionDialogKind::Bash { .. });
                let selected = perm.selected;

                if is_bash && selected == 2 {
                    // Prefix allow selected via Enter on Bash dialog.
                    let first_word = perm
                        .input_summary
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if !first_word.is_empty() {
                        self.bash_prefix_allowlist.insert(first_word.clone());
                        self.notifications.push(Notification::new(
                            format!("Prefix '{first_word}' added — future commands auto-approved"),
                            crate::widgets::notification::NotificationLevel::Info,
                        ));
                    }
                    if let Some(perm) = self.pending_permission.take() {
                        let _ = perm
                            .reply_tx
                            .send(oxicode_common::PermissionResponse::AllowOnce);
                    }
                } else {
                    let response = if is_bash {
                        // Bash: 0=AllowOnce, 1=AlwaysAllow, 2=PrefixAllow(handled above),
                        //       3=Deny, 4=AlwaysDeny
                        match selected {
                            0 => oxicode_common::PermissionResponse::AllowOnce,
                            1 => oxicode_common::PermissionResponse::AlwaysAllow,
                            3 => oxicode_common::PermissionResponse::Deny,
                            _ => oxicode_common::PermissionResponse::AlwaysDeny,
                        }
                    } else {
                        match selected {
                            0 => oxicode_common::PermissionResponse::AllowOnce,
                            1 => oxicode_common::PermissionResponse::AlwaysAllow,
                            2 => oxicode_common::PermissionResponse::Deny,
                            _ => oxicode_common::PermissionResponse::AlwaysDeny,
                        }
                    };
                    if let Some(perm) = self.pending_permission.take() {
                        let _ = perm.reply_tx.send(response);
                    }
                }
            }
            // Hotkeys: y = allow once, a = allow session, n = deny, N = always deny
            // p = approve by prefix (bash only — auto-approves future commands with same first word)
            (_, KeyCode::Char('y')) => {
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm
                        .reply_tx
                        .send(oxicode_common::PermissionResponse::AllowOnce);
                }
            }
            (_, KeyCode::Char('p')) => {
                // Prefix-approve: remember the first word and auto-approve future matches.
                if let Some(ref perm) = self.pending_permission {
                    if perm.tool_name == "bash" || perm.tool_name == "Bash" {
                        let first_word = perm
                            .input_summary
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .to_string();
                        if !first_word.is_empty() {
                            self.bash_prefix_allowlist.insert(first_word.clone());
                            self.notifications.push(Notification::new(
                                format!(
                                    "Prefix '{first_word}' added — future commands auto-approved"
                                ),
                                crate::widgets::notification::NotificationLevel::Info,
                            ));
                        }
                    }
                }
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm
                        .reply_tx
                        .send(oxicode_common::PermissionResponse::AllowOnce);
                }
            }
            (_, KeyCode::Char('a')) => {
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm
                        .reply_tx
                        .send(oxicode_common::PermissionResponse::AlwaysAllow);
                }
            }
            (_, KeyCode::Char('n') | KeyCode::Esc) => {
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm.reply_tx.send(oxicode_common::PermissionResponse::Deny);
                }
            }
            (KeyModifiers::SHIFT, KeyCode::Char('N')) => {
                // "Always deny" hotkey — only for dialogs that have this option.
                if let Some(ref perm) = self.pending_permission {
                    if matches!(
                        perm.kind,
                        crate::widgets::PermissionDialogKind::FileRead { .. }
                    ) {
                        return; // FileRead has no "Always deny" option
                    }
                }
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm
                        .reply_tx
                        .send(oxicode_common::PermissionResponse::AlwaysDeny);
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if let Some(perm) = self.pending_permission.take() {
                    let _ = perm.reply_tx.send(oxicode_common::PermissionResponse::Deny);
                }
                self.handle_ctrl_c().await;
            }
            _ => {}
        }
    }
}
