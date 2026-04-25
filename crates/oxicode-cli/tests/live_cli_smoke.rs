//! CLI smoke tests for the oxicode binary.
//!
//! Tests that require API access are gated behind `#[ignore]`.
//! Run with: `cargo test -p oxicode-cli --test live_cli_smoke -- --nocapture`

use std::process::Command;

/// Get the path to the compiled binary.
fn binary_path() -> String {
    // Build the binary first.
    let status = Command::new("cargo")
        .args(["build", "-p", "oxicode-cli"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("Failed to build oxicode-cli");
    assert!(status.success(), "cargo build failed");

    // The binary name is "oxicode" (from [[bin]] in Cargo.toml).
    let target_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target")
        .join("debug")
        .join("oxicode");

    assert!(
        target_dir.exists(),
        "Binary not found at: {}",
        target_dir.display()
    );
    target_dir.to_string_lossy().to_string()
}

#[test]
fn test_cli_version() {
    let bin = binary_path();
    let output = Command::new(&bin)
        .arg("--version")
        .output()
        .expect("Failed to run --version");

    assert!(
        output.status.success(),
        "oxicode --version should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("oxicode"),
        "Version output should contain 'oxicode', got: {stdout}"
    );
}

#[test]
fn test_cli_help() {
    let bin = binary_path();
    let output = Command::new(&bin)
        .arg("--help")
        .output()
        .expect("Failed to run --help");

    assert!(
        output.status.success(),
        "oxicode --help should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("OxiCode"),
        "Help should mention 'OxiCode', got: {stdout}"
    );
    assert!(stdout.contains("--model"), "Help should list --model flag");
    assert!(
        stdout.contains("--prompt"),
        "Help should list --prompt flag"
    );
}

#[test]
fn test_cli_completions() {
    let bin = binary_path();
    let output = Command::new(&bin)
        .args(["--completions", "bash"])
        .output()
        .expect("Failed to run --completions bash");

    assert!(
        output.status.success(),
        "oxicode --completions bash should exit 0"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.is_empty(),
        "Shell completions should produce output"
    );
}

#[tokio::test]
#[ignore = "live CLI test — requires built binary and ANTHROPIC_API_KEY"]
async fn test_cli_single_prompt_text() {
    let bin = binary_path();
    let output = Command::new(&bin)
        .args([
            "-p",
            "Say exactly 'hello oxicode' and nothing else",
            "--no-onboard",
        ])
        .envs(
            std::env::vars()
                .filter(|(k, _)| k.starts_with("ANTHROPIC_") || k == "PATH" || k == "HOME"),
        )
        .output()
        .expect("Failed to run single prompt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "Single prompt should exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.is_empty(),
        "Should produce output for prompt.\nstderr: {stderr}"
    );
}

#[tokio::test]
#[ignore = "live CLI test — requires built binary and ANTHROPIC_API_KEY"]
async fn test_cli_single_prompt_json_output() {
    let bin = binary_path();
    let output = Command::new(&bin)
        .args(["-p", "Say hello", "--output", "json", "--no-onboard"])
        .envs(
            std::env::vars()
                .filter(|(k, _)| k.starts_with("ANTHROPIC_") || k == "PATH" || k == "HOME"),
        )
        .output()
        .expect("Failed to run JSON prompt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "JSON prompt should exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // JSON output should be NDJSON (one JSON object per line).
    let non_empty_lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(
        !non_empty_lines.is_empty(),
        "JSON output should have at least one line.\nstderr: {stderr}"
    );

    // Each line should be valid JSON.
    for line in &non_empty_lines {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(
            parsed.is_ok(),
            "Each line should be valid JSON, got: {line}"
        );
    }
}

#[test]
fn test_cli_no_api_key_exits_cleanly() {
    let bin = binary_path();

    // Run without any API credentials — should error gracefully, not panic.
    let output = Command::new(&bin)
        .args(["-p", "hello", "--no-onboard"])
        .env_clear()
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .expect("Failed to run without API key");

    // Should exit with non-zero (no API key configured).
    // Just verify it doesn't panic (exit code 101 = panic).
    let code = output.status.code().unwrap_or(-1);
    assert_ne!(
        code,
        101,
        "Should not panic without API key. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ═══════════════════════════════════════════════════════════════════
// Additional binary integration tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore = "live CLI test — requires built binary and ANTHROPIC_API_KEY"]
async fn test_cli_single_prompt_math() {
    let bin = binary_path();
    let output = Command::new(&bin)
        .args([
            "-p",
            "What is 2+2? Reply with just the number.",
            "--no-onboard",
        ])
        .envs(
            std::env::vars()
                .filter(|(k, _)| k.starts_with("ANTHROPIC_") || k == "PATH" || k == "HOME"),
        )
        .output()
        .expect("Failed to run math prompt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Math prompt should exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains('4'), "Should contain 4, got: {stdout}");
}

#[tokio::test]
#[ignore = "live CLI test — requires built binary and ANTHROPIC_API_KEY"]
async fn test_cli_json_has_session_events() {
    let bin = binary_path();
    let output = Command::new(&bin)
        .args(["-p", "Say OK", "--output", "json", "--no-onboard"])
        .envs(
            std::env::vars()
                .filter(|(k, _)| k.starts_with("ANTHROPIC_") || k == "PATH" || k == "HOME"),
        )
        .output()
        .expect("Failed to run JSON events prompt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Should exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Parse NDJSON and verify event types.
    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    assert!(!events.is_empty(), "Should have NDJSON events");

    // Should have session_start event.
    let has_start = events.iter().any(|e| {
        e.get("type")
            .and_then(|t| t.as_str())
            .is_some_and(|t| t == "session_start")
    });
    assert!(has_start, "Should have session_start event");

    // Should have session_end event.
    let has_end = events.iter().any(|e| {
        e.get("type")
            .and_then(|t| t.as_str())
            .is_some_and(|t| t == "session_end")
    });
    assert!(has_end, "Should have session_end event");
}

#[test]
fn test_cli_completions_zsh() {
    let bin = binary_path();
    let output = Command::new(&bin)
        .args(["--completions", "zsh"])
        .output()
        .expect("Failed to run --completions zsh");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "Zsh completions should produce output");
}

#[test]
fn test_cli_completions_fish() {
    let bin = binary_path();
    let output = Command::new(&bin)
        .args(["--completions", "fish"])
        .output()
        .expect("Failed to run --completions fish");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "Fish completions should produce output");
}

#[test]
fn test_cli_man_page_generation() {
    let bin = binary_path();
    let output = Command::new(&bin)
        .arg("--man-page")
        .output()
        .expect("Failed to run --man-page");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "Man page should produce output");
    assert!(
        stdout.contains("oxicode") || stdout.contains("OXICODE"),
        "Man page should reference oxicode"
    );
}

#[test]
fn test_cli_invalid_flag_exits_nonzero() {
    let bin = binary_path();
    let output = Command::new(&bin)
        .arg("--invalid-flag-xyz")
        .output()
        .expect("Failed to run invalid flag");

    assert!(
        !output.status.success(),
        "Invalid flag should exit non-zero"
    );
}
