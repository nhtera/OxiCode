# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

OxiCode — a Rust CLI agent for software engineering, full-parity port of Claude Code. 17-crate Cargo workspace producing a single `oxicode` binary with a ratatui TUI, 49 built-in tools, multi-provider LLM support, and MCP client.

## Build & Development Commands

```bash
# Type check entire workspace
cargo check --workspace

# Run all tests (~191 unit tests)
cargo test --workspace

# Run a single crate's tests
cargo test --package oxicode-tools

# Run a single test by name
cargo test --package oxicode-core -- test_name

# Lint (CI runs with -D warnings via RUSTFLAGS)
cargo clippy --workspace

# Format check (CI enforced)
cargo fmt --check

# Format fix
cargo fmt --all

# Build release binary
cargo build --release

# Build with all optional features
cargo build --features full

# Run the binary (dev mode)
cargo run --bin oxicode

# Benchmarks
cargo bench --package oxicode-cli
```

CI pipeline order: `fmt` → `clippy` + `test` (parallel, 3 OS) → `build` (5 targets). CI sets `RUSTFLAGS="-D warnings"`.

## Workspace Architecture

Binary entry point: `crates/oxicode-cli/src/main.rs` → `oxicode` binary.

**Request flow:** User input → `QueryEngine` (oxicode-core) → `LlmProvider` (oxicode-api) → stream response → `ToolRegistry` dispatches tool calls → `PermissionPipeline` gates execution → results feed back to LLM.

### Crate Dependency Layers (top → bottom)

```
oxicode-cli          ← binary, wires everything together
  ├─ oxicode-core    ← QueryEngine: multi-turn loop, conversation, tool dispatch
  ├─ oxicode-tui     ← ratatui terminal UI, markdown rendering, themes
  ├─ oxicode-agents  ← multi-agent system, subagent spawning
  ├─ oxicode-skills  ← skill discovery + execution
  ├─ oxicode-plugins ← subprocess plugin system
  │
  ├─ oxicode-api     ← LlmProvider trait + implementations (Anthropic, OpenAI-compat, Bedrock, Vertex)
  ├─ oxicode-tools   ← Tool trait + ToolRegistry + 49 tool implementations
  ├─ oxicode-permissions ← 6-layer PermissionPipeline
  ├─ oxicode-mcp     ← MCP client (stdio, SSE, streamable HTTP, WebSocket)
  ├─ oxicode-context ← BudgetManager, token counting, compaction strategies
  ├─ oxicode-tasks   ← background task management
  ├─ oxicode-hooks   ← 26 lifecycle hook events
  │
  ├─ oxicode-config  ← TOML + env + CLAUDE.md config loading
  ├─ oxicode-session ← session persistence (save/load/resume)
  ├─ oxicode-state   ← StateStore: centralized AppState via watch channels
  └─ oxicode-common  ← shared types (Message, ContentBlock, Role, OxiResult, OxiError)
```

### Key Traits & Patterns

- **`LlmProvider`** (`oxicode-api/src/provider.rs`): `async fn stream_message(&self, request: MessageRequest) -> OxiResult<EventStream>`. Implement this to add a new LLM provider.
- **`Tool`** (`oxicode-tools/src/tool_trait.rs`): `async fn execute(&self, input: Value, ctx: &ToolContext) -> OxiResult<ToolResult>`. Each tool file (e.g., `bash.rs`, `file_read.rs`) implements this trait and registers with `ToolRegistry`.
- **`PermissionPipeline`** (`oxicode-permissions/src/pipeline.rs`): 6 layers evaluated in order — (1) safe allowlist, (2) command security hard-deny, (3) mode bypass, (4) dangerous pattern detection, (5) user rules, (6) default ask. Returns `Allow | Deny | Ask`.
- **`QueryEngine`** (`oxicode-core/src/query_engine.rs`): Owns the provider, tool registry, permission pipeline, state store, and budget manager. Runs the multi-turn loop (max 50 tool turns).
- **`StateStore`** (`oxicode-state`): Centralized `AppState` shared across crates via `tokio::sync::watch` channels.
- **`BudgetManager`** (`oxicode-context/src/budget.rs`): Tracks context window usage, triggers compaction strategies.

### Feature Flags (oxicode-cli)

| Flag | Purpose |
|------|---------|
| `voice` | Microphone input + Whisper transcription |
| `bridge` | WebSocket remote mode |
| `telemetry-otlp` | OpenTelemetry event collection |
| `remote`, `dream`, `teammate` | Extended task/tool capabilities |
| `full` | voice + bridge + telemetry-otlp |
| `all` | Every feature |

## Code Conventions

- **Rust edition 2021**, MSRV 1.80
- `unsafe_code = "forbid"` workspace-wide
- Clippy `all` + `pedantic` at warn level (with specific allows — see `Cargo.toml [workspace.lints.clippy]`)
- Error types: `OxiResult<T>` / `OxiError` from oxicode-common; use `thiserror` for crate-specific errors, `anyhow` in CLI
- Async runtime: tokio (full features)
- All tool implementations live in `crates/oxicode-tools/src/` as individual files named after the tool

## Configuration

OxiCode reads config from (in precedence order):
1. Environment variables (`ANTHROPIC_API_KEY`, `OXICODE_MODEL`, etc.)
2. `OXICODE.md` / `CLAUDE.md` — project-level instructions
3. `~/.oxicode/settings.toml` — global settings

## Release Profiles

- `release`: thin LTO, strip symbols, codegen-units=1, opt-level=3, panic=abort
- `release-small`: inherits release, opt-level=z, fat LTO (size-optimized)
- `profiling`: inherits release, keeps debug info
