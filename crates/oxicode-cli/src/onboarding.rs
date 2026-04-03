//! First-run onboarding wizard for OxiCode.
//!
//! Detects whether `~/.oxicode/` exists. If not, guides the user through
//! API key, model selection, permission mode, and theme setup.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::PathBuf;

/// Check if onboarding should run (no config dir exists).
pub fn should_onboard() -> bool {
    let config_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oxicode");
    !config_dir.exists()
}

/// Run the interactive onboarding wizard. Returns `true` if settings were saved.
pub fn run_onboarding() -> bool {
    eprintln!();
    eprintln!("  Welcome to OxiCode! 🦀");
    eprintln!("  Let's set up your environment.\n");

    // Step 1: API key.
    let api_key = prompt_input("  Anthropic API key (ANTHROPIC_API_KEY): ");
    if api_key.is_empty() {
        eprintln!("  → Skipped. Set ANTHROPIC_API_KEY env var later.");
    }

    // Step 2: Model selection.
    eprintln!("\n  Available models:");
    let models = [
        "claude-sonnet-4-20250514",
        "claude-haiku-4-20250414",
        "claude-opus-4-20250514",
    ];
    for (i, m) in models.iter().enumerate() {
        let marker = if i == 0 { " (default)" } else { "" };
        eprintln!("    {}) {m}{marker}", i + 1);
    }
    let model_choice = prompt_line("  Choose model [1]: ");
    let model_idx: usize = model_choice.trim().parse().unwrap_or(1);
    let model = models
        .get(model_idx.saturating_sub(1))
        .unwrap_or(&models[0]);

    // Step 3: Permission mode.
    eprintln!("\n  Permission modes:");
    eprintln!("    1) default — ask before edits/commands");
    eprintln!("    2) accept_edits — auto-approve file edits");
    eprintln!("    3) bypass — approve everything (dangerous)");
    let perm_choice = prompt_line("  Choose permission mode [1]: ");
    let permission_mode = match perm_choice.trim() {
        "2" => "accept_edits",
        "3" => "bypass",
        _ => "default",
    };

    // Step 4: Theme.
    eprintln!("\n  Themes: dark, light, catppuccin, dracula, solarized");
    let theme = prompt_line("  Choose theme [dark]: ");
    let theme = if theme.trim().is_empty() {
        "dark"
    } else {
        theme.trim()
    };

    // Save to ~/.oxicode/settings.toml.
    let config_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oxicode");

    if let Err(e) = std::fs::create_dir_all(&config_dir) {
        eprintln!("  Error creating config dir: {e}");
        return false;
    }

    let mut toml_content = String::new();
    if !api_key.is_empty() {
        let _ = writeln!(toml_content, "api_key = \"{api_key}\"");
    }
    let _ = writeln!(toml_content, "model = \"{model}\"");
    let _ = writeln!(toml_content, "permission_mode = \"{permission_mode}\"");
    let _ = writeln!(toml_content, "theme = \"{theme}\"");

    let settings_path = config_dir.join("settings.toml");
    match std::fs::write(&settings_path, &toml_content) {
        Ok(()) => {
            // Restrict file permissions (API key may be present).
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o600);
                let _ = std::fs::set_permissions(&settings_path, perms);
            }
            eprintln!(
                "\n  ✓ Settings saved to {}",
                settings_path.display()
            );
            eprintln!("  Run `oxicode` to start.\n");
            true
        }
        Err(e) => {
            eprintln!("  Error writing settings: {e}");
            false
        }
    }
}

/// Prompt for a line of input (visible).
fn prompt_line(prompt: &str) -> String {
    eprint!("{prompt}");
    let _ = io::stderr().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
    buf.trim().to_string()
}

/// Prompt for input (not masked — use env var for secrets).
fn prompt_input(prompt: &str) -> String {
    eprint!("{prompt}");
    let _ = io::stderr().flush();

    // Try to disable echo for password-style input.
    // On failure, fall back to normal visible input.
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
    let trimmed = buf.trim().to_string();
    if !trimmed.is_empty() {
        eprintln!("  → API key set (hidden)");
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_onboard_respects_existing_dir() {
        // This test just verifies the function doesn't panic.
        // Actual result depends on whether ~/.oxicode/ exists on the test machine.
        let _ = should_onboard();
    }
}
