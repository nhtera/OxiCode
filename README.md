# OxiCode

A Rust-powered CLI agent for software engineering — full-parity port of Claude Code.

## Features

- **Multi-provider LLM** — Anthropic (Claude), with extensible provider trait
- **49 built-in tools** — file operations, bash, git, grep, glob, agents, MCP, and more
- **6-layer permission pipeline** — safe allowlist, mode checks, command security, dangerous pattern detection, user rules, default prompting
- **Ratatui TUI** — split panes, markdown rendering, syntax highlighting, themes, mouse support
- **MCP client** — stdio, SSE, streamable HTTP, WebSocket transports
- **Multi-agent system** — spawn subagents with isolated contexts
- **Skill discovery** — auto-detect and execute skill files
- **Session persistence** — save/load/resume conversations
- **NDJSON structured output** — `--output json` for IDE extensions, CI/CD, automation
- **105+ slash commands** — /help, /model, /session, /commit, /plan, /team, and more
- **Shell completions** — bash, zsh, fish, powershell
- **Cross-platform** — macOS (arm64, x86_64), Linux (x86_64, arm64), Windows (x86_64)

## Installation

### Quick install (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/nicktien007/oxicode/main/scripts/install.sh | bash
```

### From source

```bash
git clone https://github.com/nicktien007/oxicode.git
cd oxicode
cargo install --path crates/oxicode-cli
```

### Homebrew (macOS)

```bash
brew tap nicktien007/oxicode
brew install oxicode
```

## Quick Start

```bash
# Set your API key
export ANTHROPIC_API_KEY="sk-ant-..."

# Interactive mode
oxicode

# Single prompt (non-interactive)
oxicode -p "Explain the main function in src/main.rs"

# NDJSON output for scripting
oxicode -p "List all TODO comments" --output json | jq '.type'
```

## Configuration

OxiCode reads configuration from:

1. `~/.oxicode/settings.toml` — global settings
2. `OXICODE.md` / `CLAUDE.md` — project-level instructions (OXICODE.md takes precedence)
3. Environment variables (`ANTHROPIC_API_KEY`, `OXICODE_MODEL`, etc.)

### settings.toml

```toml
model = "claude-sonnet-4-20250514"
max_tokens = 16384
permission_mode = "default"  # default | bypass | approval_only
```

### Permission modes

| Mode | Behavior |
|------|----------|
| `default` | Ask for non-readonly tools, auto-allow reads |
| `bypass` | Trust everything (use with caution) |
| `approval_only` | Ask for every non-readonly operation |

## Shell Completions

```bash
# Bash
oxicode --completions bash > ~/.local/share/bash-completion/completions/oxicode

# Zsh
oxicode --completions zsh > ~/.zfunc/_oxicode

# Fish
oxicode --completions fish > ~/.config/fish/completions/oxicode.fish

# PowerShell
oxicode --completions powershell > oxicode.ps1
```

## Man Page

```bash
oxicode --man-page > /usr/local/share/man/man1/oxicode.1
```

## Architecture

17 Cargo crates in a workspace:

| Crate | Purpose |
|-------|---------|
| `oxicode-cli` | Binary entry point, CLI args |
| `oxicode-core` | Query engine, conversation, system prompt |
| `oxicode-api` | Multi-provider LLM abstraction |
| `oxicode-tools` | 49 tools + registry |
| `oxicode-permissions` | 6-layer permission pipeline |
| `oxicode-tui` | Ratatui terminal UI |
| `oxicode-config` | TOML + env + CLAUDE.md config |
| `oxicode-session` | Session persistence |
| `oxicode-hooks` | 26 lifecycle events |
| `oxicode-mcp` | MCP client (4 transports) |
| `oxicode-agents` | Multi-agent system |
| `oxicode-skills` | Skill discovery + execution |
| `oxicode-context` | 5-layer context defense |
| `oxicode-tasks` | Background task management |
| `oxicode-plugins` | Subprocess plugin system |
| `oxicode-state` | Centralized app state |
| `oxicode-common` | Shared types, errors, utils |

## Development

```bash
# Check
cargo check --workspace

# Test
cargo test --workspace

# Clippy
cargo clippy --workspace

# Format
cargo fmt --check

# Benchmarks
cargo bench --package oxicode-cli

# Build release
cargo build --release
```

## License

MIT — see [LICENSE](LICENSE).
