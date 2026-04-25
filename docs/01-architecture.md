# System Architecture

OxiCode is a Rust CLI agent for software engineering, providing full feature parity with Claude Code. It's a 17-crate workspace producing a single `oxicode` binary with a ratatui TUI, 49 built-in tools, multi-provider LLM support (Anthropic, OpenAI, Bedrock, Vertex), and MCP client capabilities.

## Overview

**What is OxiCode?**

A terminal-based AI coding assistant that:
- Accepts natural language prompts from the user
- Calls LLM providers (Anthropic Claude as default) to generate responses
- Extracts tool calls and executes them (file I/O, bash, web access, etc.)
- Feeds results back to the LLM for multi-turn conversations
- Persists conversations to disk for resumption
- Runs in TUI (ratatui), single-prompt, daemon, or MCP server modes

**Architecture highlights:**
- Multi-provider LLM abstraction (swap providers at runtime)
- Permission pipeline guards all tool execution
- Context window defense via automatic compaction
- Streaming markdown rendering in TUI
- 26 lifecycle hooks for extensibility
- MCP client for third-party tools
- Subagent spawning for task delegation

**Target parity:** Full feature coverage of Claude Code (Cursor's AI agent) in a standalone Rust binary.

---

## Crate Dependency DAG

### Four Layers (top → bottom)

```
LAYER 4: BINARY ENTRY
┌─────────────────┐
│  oxicode-cli    │ 1200 LOC (main.rs, CLI modes, event loop)
└─────────────────┘
          ↓
LAYER 3: ENGINE & PLUGINS
┌──────────────────┬─────────────────┬──────────────────┐
│ oxicode-core     │  oxicode-tui    │  oxicode-agents  │
│ 800 LOC          │  5200 LOC       │  600 LOC         │
│ QueryEngine      │ ratatui app     │ team management  │
├──────────────────┼─────────────────┼──────────────────┤
│ oxicode-skills   │                 │ oxicode-plugins  │
│ 400 LOC          │                 │  200 LOC         │
│ skill discovery  │                 │ subprocess tools │
└──────────────────┴─────────────────┴──────────────────┘
          ↓
LAYER 2: SERVICES
┌──────────────────┬─────────────────┬──────────────────┐
│ oxicode-api      │ oxicode-tools   │ oxicode-perms    │
│ 600 LOC          │  3800 LOC       │  600 LOC         │
│ LlmProvider,     │ Tool registry   │ PermissionPipe   │
│ Anthropic, OAI,  │ 49 tools        │ line 6-layer     │
│ Bedrock, Vertex  │                 │ decision tree    │
├──────────────────┼─────────────────┼──────────────────┤
│ oxicode-mcp      │ oxicode-context │ oxicode-tasks    │
│ 800 LOC          │  700 LOC        │  400 LOC         │
│ MCP client       │ BudgetManager   │ TaskManager      │
│ stdio, SSE, WS   │ token counting  │ background jobs  │
├──────────────────┼─────────────────┼──────────────────┤
│ oxicode-hooks    │                 │                  │
│ 300 LOC          │                 │                  │
│ 26 lifecycle     │                 │                  │
│ hook events      │                 │                  │
└──────────────────┴─────────────────┴──────────────────┘
          ↓
LAYER 1: FOUNDATION
┌──────────────────┬─────────────────┬──────────────────┐
│ oxicode-config   │ oxicode-session │ oxicode-state    │
│ 300 LOC          │  500 LOC        │  400 LOC         │
│ TOML loading     │ session I/O     │ StateStore,      │
│ CLAUDE.md        │ save/load       │ AppState,        │
│ env var fallback │ resume support  │ watch channels   │
├──────────────────┼─────────────────┼──────────────────┤
│ oxicode-common   │                 │                  │
│ 400 LOC          │                 │                  │
│ Message types    │                 │                  │
│ ContentBlock     │                 │                  │
│ Role, OxiError   │                 │                  │
└──────────────────┴─────────────────┴──────────────────┘

Total: ~51,600 LOC Rust | 17 crates | ~191 unit tests
```

### Dependency Graph (alphabetical by layer)

**Layer 3 → Layer 2 + Layer 1:**
- `oxicode-agents` → oxicode-core, oxicode-tools, oxicode-state
- `oxicode-cli` → all layers (wires everything)
- `oxicode-core` → oxicode-api, oxicode-tools, oxicode-permissions, oxicode-context, oxicode-state, oxicode-common
- `oxicode-plugins` → oxicode-tools, oxicode-common
- `oxicode-skills` → oxicode-common
- `oxicode-tui` → oxicode-core, oxicode-state, oxicode-common

**Layer 2 → Layer 1:**
- `oxicode-api` → oxicode-common
- `oxicode-context` → oxicode-api, oxicode-common
- `oxicode-hooks` → oxicode-common
- `oxicode-mcp` → oxicode-tools, oxicode-common
- `oxicode-permissions` → oxicode-tools, oxicode-common
- `oxicode-tasks` → oxicode-common
- `oxicode-tools` → oxicode-common

**Layer 1 (no outbound dependencies):**
- `oxicode-common`, `oxicode-config`, `oxicode-session`, `oxicode-state`

(source: crates/*/Cargo.toml dependencies)

---

## Startup Sequence

**Entry point:** `crates/oxicode-cli/src/main.rs` → `async fn main()`

```
main() {
  1. Parse CLI args (clap)
     ├─ --model: LLM model name (defaults to OXICODE_MODEL env or config)
     ├─ --prompt: Single-prompt mode (non-interactive, exit after response)
     ├─ --session: Resume session by ID
     ├─ --output: text (TUI) or json (NDJSON structured)
     ├─ --mcp: Run as MCP server over stdio
     ├─ --daemon: Run as TCP daemon for IDE connections
     ├─ --server: Run as JSON-RPC server for IDE integration
     ├─ --agent-mode: Subagent mode (read config from stdin)
     └─ --no-onboard: Skip first-run setup wizard

  2. Fast-exit paths (return early):
     ├─ if --agent-mode: run_agent_mode() → read stdin, execute, write JSON output
     ├─ if --completions: generate shell completions, exit
     ├─ if --man-page: generate man page, exit
     └─ if --mcp: run_mcp_server_mode() → expose tools via stdio JSON-RPC

  3. Load configuration (in order of precedence):
     ├─ Environment variables (ANTHROPIC_API_KEY, OXICODE_MODEL, etc.)
     ├─ CLAUDE.md (global project-level instructions)
     ├─ OXICODE.md (project-specific overrides)
     └─ ~/.oxicode/settings.toml (user defaults)

  4. Setup tracing:
     ├─ Create log directory (~/.local/share/oxicode/)
     ├─ Open log file (oxicode.log)
     └─ Initialize tracing_subscriber with env filter

  5. Initialize LLM provider:
     ├─ Create ProviderRouter from env (OAuth token if available)
     ├─ Resolve provider by model name (Anthropic, OpenAI, Bedrock, Vertex)
     └─ Validate provider connectivity (fail early if API key missing)

  6. Discover and load skills:
     ├─ User skills: ~/.oxicode/skills/
     ├─ Project skills: .oxicode/skills/
     ├─ Build skills_prompt (system prompt injection)
     └─ Create SkillExecutor

  7. Assemble system prompt:
     ├─ Global CLAUDE.md instructions
     ├─ Project CLAUDE.md/OXICODE.md overrides
     ├─ Skills prompt (discovered tools)
     └─ Project memory (reads ~/.oxicode/projects/{key}/memory/MEMORY.md + extra .md files, capped at 100 KB)

  8. Initialize state:
     ├─ Create StateStore (centralized AppState with watch channels)
     ├─ Load or create Session (save/resume support)
     ├─ Create ToolRegistry (49 built-in tools)
     ├─ Create PermissionPipeline (6-layer decision tree)
     └─ Create ToolContext (shared file/task/MCP state)

  9. Initialize MCP servers:
     ├─ Read ~/.config/oxicode/mcp_servers.toml
     ├─ Start each configured MCP server (stdio, SSE, HTTP)
     └─ Register MCP tools in tool registry

  10. Construct QueryEngine:
      ├─ Owned by Arc<>, shared across tasks
      ├─ Stores provider, registry, permissions, state, budget manager
      └─ Ready for multi-turn execution

  11. Run application mode:
      ├─ if --prompt: run_single_prompt() → one turn, JSON/text output, exit
      ├─ if --server: run_server() → JSON-RPC event loop
      ├─ if --daemon: daemon_listener() → TCP listener for IDE
      ├─ if --bridge: bridge mode → WebSocket server for cloud (--features bridge)
      └─ else: run_tui() → interactive ratatui terminal UI

  12. Graceful shutdown:
      ├─ Save session to disk
      ├─ Shutdown MCP servers
      └─ Wait for engine task (5s timeout, then abort)
}
```

(source: crates/oxicode-cli/src/main.rs)

---

## Request Lifecycle

**End-to-end flow for one user input → LLM response → tool execution → next turn:**

```
USER TYPES PROMPT IN TUI
           ↓
    TUI emits UiEvent::UserInput { text, images }
           ↓
    UiEvent → mpsc channel to engine task
           ↓
MAIN THREAD (engine_handle task):
  ├─ Convert images to base64 (if pasted)
  ├─ Create Message::user(text) + image blocks
  ├─ Push to conversation & state_store (for TUI rendering)
  └─ Call engine.execute_turn_with_cancel()
           ↓
QUERYENGINE::EXECUTE_TURN:
  1. Pre-turn checks:
     ├─ Increment turn count (max 50)
     ├─ Check cancel flag (user pressed Ctrl+C)
     └─ Apply budget defense (compaction if needed)

  2. Build MessageRequest:
     ├─ model = current model name
     ├─ messages = conversation.api_messages() (with system prompt)
     ├─ max_tokens = engine.max_tokens
     ├─ tools = serialize ToolRegistry to JSON schema
     └─ stream = true (always streaming)

  3. Call LlmProvider::stream_message(request):
     ├─ Provider creates SSE connection to API
     ├─ Returns EventStream (futures::Stream<Item=OxiResult<StreamEvent>>)
     └─ Stream yields events as they arrive

  4. Process stream events (polling loop):
     ├─ while let Some(event) = stream.next().await:
     │   ├─ TextDelta(text) → append to assistant_msg.content
     │   │                    emit TurnEvent::TextDelta → TUI renders live
     │   │
     │   ├─ ToolUseStart { id, name, input }
     │   │   ├─ Create ContentBlock::ToolUse
     │   │   ├─ Emit TurnEvent::ToolUseStart → TUI shows "Calling tool_name..."
     │   │   └─ Save for later execution
     │   │
     │   ├─ ToolUseEnd { id }
     │   │   ├─ Tool call fully received (multi-chunk input)
     │   │   └─ Ready to execute
     │   │
     │   ├─ UsageUpdate { input, output }
     │   │   └─ Update assistant_msg.usage
     │   │
     │   ├─ MessageDelta { stop_reason }
     │   │   ├─ Model signaling completion (EndTurn, ToolUse, MaxTokens)
     │   │   └─ Emit TurnEvent::TurnEnd
     │   │
     │   └─ ErrorEvent(e)
     │       └─ Stream error (retry or abort)
     │
     └─ Finish when stream closed or error

  5. Extract tool calls:
     ├─ Iterate over assistant_msg.content blocks
     ├─ For each ContentBlock::ToolUse:
     │   ├─ Parse input JSON
     │   ├─ Emit TurnEvent::ToolUseStart → TUI
     │   └─ Queue for execution
     └─ If no tool calls, or stop_reason == EndTurn, skip to step 8

  6. Permission pipeline (for each tool call):
     ├─ permission_pipeline.check(tool_name, input_json)
     │   ├─ Evaluate 6 layers (safe_allowlist, hard_deny, bypass, ...)
     │   └─ Returns Allow | Deny(reason) | Ask
     │
     ├─ If Deny:
     │   ├─ Create ContentBlock::ToolResult { is_error: true, content: reason }
     │   └─ Append to conversation (no execution)
     │
     └─ If Ask:
         ├─ Create permission_tx (oneshot channel)
         ├─ Emit CoreEvent::PermissionAsk { tool_name, input_summary, reply_tx }
         ├─ TUI shows dialog → user decides → sends PermissionResponse
         ├─ If Allow: continue to execution
         └─ If Deny: create error ToolResult, append to conversation

  7. Tool execution (for each allowed tool call):
     ├─ tool_registry.execute(tool_name, input, &tool_context)
     │   ├─ Lookup tool implementation by name
     │   ├─ Call Tool::execute(input, ctx)
     │   ├─ Tool runs (file I/O, bash, web fetch, MCP proxy, etc.)
     │   └─ Return ToolResult { content, is_error }
     │
     ├─ Create ContentBlock::ToolResult { tool_use_id, content, is_error }
     ├─ Append to conversation
     ├─ Emit TurnEvent::ToolResult → TUI renders
     └─ Continue loop (may auto-call next tool)

  8. Loop decision:
     ├─ if stop_reason == EndTurn: break (user's turn next)
     ├─ if stop_reason == ToolUse: continue (more tool calls expected)
     ├─ if stop_reason == MaxTokens: break (context full, warn user)
     ├─ if tool_error && is_critical: break (don't retry on permission error)
     └─ else: continue (more turns available)

  9. Push final assistant_msg to state_store:
     ├─ state_store.push_message(assistant_msg)
     ├─ Triggers watch subscribers (TUI re-renders)
     └─ Ready for session save

BACK IN MAIN THREAD (after execute_turn returns):
  ├─ If Ok(assistant_msg):
  │   ├─ Emit CoreEvent::MessageComplete → TUI resets is_turn_active
  │   └─ Continue event loop (wait for next UiEvent)
  │
  └─ If Err(e):
      ├─ Emit CoreEvent::Error(e.to_string()) → TUI shows error dialog
      ├─ Emit CoreEvent::MessageComplete → TUI resets
      └─ Continue event loop (allow retry)

TUI CONTINUES:
  ├─ User types next prompt
  ├─ Cycle repeats (steps 1–9)
  └─ Session auto-saved on exit
```

(source: crates/oxicode-core/src/query_engine.rs, crates/oxicode-cli/src/main.rs)

---

## Shared Infrastructure

### StateStore (Centralized AppState)

```rust
pub struct StateStore {
    state: Arc<tokio::sync::watch::Sender<AppState>>,
}

pub struct AppState {
    pub messages: Vec<Message>,
    pub current_model: String,
    pub auth_label: String,
    pub is_streaming: bool,
    pub last_error: Option<String>,
}
```

**Pattern:** All mutations go through `state_store.update(|s| { s.field = value })` or convenience methods like `push_message()`. Subscribers (TUI, session persistence) watch for changes via `state_store.watch()`.

(source: crates/oxicode-state/src/lib.rs)

### Error Handling (OxiError / OxiResult)

```rust
pub enum OxiError {
    Api { message: String, status: Option<u16>, retryable: bool },
    RateLimit { info: RateLimitInfo },
    Config(String),
    Tool { name: String, message: String },
    Permission(String),
    Session(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Tui(String),
    StreamClosed,
    Other(String),
}

pub type OxiResult<T> = Result<T, OxiError>;
```

**Pattern:** Use `?` operator to propagate errors. In CLI, convert to `anyhow::Result` for convenient error chaining. In libraries, use `thiserror` for crate-specific errors.

**Retry logic:** `is_retryable()` checks for transient errors (429, 500, 502, 503, 529). RateLimit errors include backoff duration from headers.

(source: crates/oxicode-common/src/error.rs)

### Feature Flags

Configured in root `Cargo.toml [workspace.lints]`:

| Flag | Crate | Purpose |
|------|-------|---------|
| `voice` | oxicode-cli | Microphone input + Whisper transcription |
| `bridge` | oxicode-cli | WebSocket remote mode (tokio-tungstenite) |
| `telemetry-otlp` | oxicode-cli | OpenTelemetry event collection |
| `remote` | oxicode-agents | Extended task capabilities for subagents |
| `dream` | oxicode-plugins | Dream mode (future) |
| `teammate` | oxicode-agents | Teammate agent spawning |
| `full` | oxicode-cli | Shorthand for voice + bridge + telemetry-otlp |
| `all` | oxicode-cli | All features (for testing) |

**Default:** No features (lean binary, ~30 MB release, ~100 MB debug).

(source: Cargo.toml)

### Tracing

Tracing writes to log file (not stderr, to avoid corrupting TUI):

```
~/.local/share/oxicode/oxicode.log
```

Filter: `RUST_LOG=oxicode=info` (default). Set `RUST_LOG=debug` for verbose output.

(source: crates/oxicode-cli/src/main.rs:162–169)

---

## Core Data Types

### Message

```rust
pub struct Message {
    pub id: String,                              // UUID
    pub role: Role,                              // User | Assistant | System
    pub content: Vec<ContentBlock>,              // Text, Image, ToolUse, ToolResult, Thinking
    pub model: Option<String>,                   // e.g., "claude-sonnet-4-20250514"
    pub stop_reason: Option<StopReason>,         // EndTurn | ToolUse | MaxTokens | StopSequence
    pub created_at: DateTime<Utc>,               // Timestamp
    pub usage: Option<Usage>,                    // input_tokens, output_tokens, cache_*
}

pub enum Role { User, Assistant, System }
```

(source: crates/oxicode-common/src/types.rs:63–74)

### ContentBlock

```rust
pub enum ContentBlock {
    Text { text: String },
    Image { source: ImageSource },              // base64-encoded
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    Thinking { thinking: String },              // Extended thinking (not sent to API)
}
```

(source: crates/oxicode-common/src/types.rs:28–51)

### StopReason

```rust
pub enum StopReason {
    EndTurn,        // Natural conversation end
    ToolUse,        // Tool calls were made (more turns expected)
    MaxTokens,      // Context window limit reached
    StopSequence,   // Custom stop sequence triggered
}
```

(source: crates/oxicode-common/src/types.rs:126–134)

### Usage

```rust
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_creation_input_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
}
```

(source: crates/oxicode-common/src/types.rs:137–143)

---

## Build & CI Pipeline

### Workspace Layout

```
oxicode/
├── Cargo.toml                      # Workspace root, shared deps, lints, profiles
├── crates/
│   ├── oxicode-cli/
│   ├── oxicode-core/
│   ├── oxicode-tui/
│   ├── oxicode-api/
│   ├── ... (17 total)
│   └── oxicode-common/
├── tests/                          # Integration tests
├── .github/workflows/
│   └── ci.yml                      # GitHub Actions
└── docs/
    └── *.md                        # Design docs
```

### Release Profiles (Cargo.toml)

```toml
[profile.release]
lto = "thin"              # Faster builds, good optimization
strip = true              # Remove debug symbols (~20% size reduction)
codegen-units = 1         # Maximum optimization (slower compile)
opt-level = 3             # Full speed optimization
panic = "abort"           # No unwinding tables (~5% smaller)

[profile.release-small]
inherits = "release"
opt-level = "z"           # Size over speed
lto = "fat"               # Maximum size reduction
```

**Build commands:**
```bash
cargo build --release                # ~30 MB binary
cargo build --release --features full # ~35 MB binary (voice + bridge + telemetry)
cargo build --release-small           # ~20 MB binary (size optimized)
```

(source: Cargo.toml:160–177)

### Feature Flags Table

Configured in `Cargo.toml [workspace.lints.clippy]`:

```rust
all = { level = "warn", priority = -1 }        // All clippy lints
pedantic = { level = "warn", priority = -1 }   // Strict lints

// Allowed patterns (for flexibility):
module_name_repetitions = "allow"
must_use_candidate = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
return_self_not_must_use = "allow"
struct_field_names = "allow"
enum_variant_names = "allow"
unused_self = "allow"
unnecessary_literal_bound = "allow"
needless_pass_by_value = "allow"
cast_possible_truncation = "allow"
doc_markdown = "allow"
```

(source: Cargo.toml:30–48)

### CI Pipeline

GitHub Actions (`.github/workflows/ci.yml`):

```
1. Checkout code
2. Format check:
   cargo fmt --all -- --check

3. Test (3 OS parallel: linux, macos, windows):
   RUSTFLAGS="-D warnings" cargo test --workspace

4. Lint:
   RUSTFLAGS="-D warnings" cargo clippy --workspace -- -D warnings

5. Build (5 targets):
   cargo build --release --target x86_64-unknown-linux-gnu
   cargo build --release --target x86_64-apple-darwin
   cargo build --release --target aarch64-apple-darwin
   cargo build --release --target x86_64-pc-windows-msvc
   cargo build --release --features full
```

**Order:** fmt → test + clippy (parallel) → build (parallel).

**Fail fast:** If format or lint fails, build doesn't run.

---

## Integration Points

### Session Persistence

Session saved to `~/.local/share/oxicode/sessions/{session_id}.json`.

```rust
pub fn save_session(session: &Session, dir: Option<&str>) -> Result<()>
pub fn load_session(session_id: &str, dir: Option<&str>) -> Result<Session>
pub fn list_sessions() -> Result<Vec<SessionSummary>>
```

TUI auto-saves on exit. Resume with `--session <id>`.

(source: crates/oxicode-session/src/lib.rs)

### MCP Server Integration

OxiCode acts as MCP client. Configured servers in `~/.config/oxicode/mcp_servers.toml`:

```toml
[[servers]]
name = "filesystem"
type = "stdio"
command = "python"
args = ["-m", "mcp.server.filesystem", "--directory", "/workspace"]

[[servers]]
name = "github"
type = "sse"
url = "http://localhost:3001/sse"
```

Each server's tools are registered in tool registry with `mcp_{server}_{tool}` naming.

(source: crates/oxicode-mcp/src/lib.rs)

### System Prompt Injection

Three levels (in order of precedence):

1. **Global:** `~/.claude/CLAUDE.md` (user's universal instructions)
2. **Project:** `CLAUDE.md` or `OXICODE.md` in current directory
3. **Skills:** Auto-generated from discovered skills

Final system prompt = global + project + skills + project memory (injected at startup).

(source: crates/oxicode-config/src/lib.rs, crates/oxicode-core/src/system_prompt.rs)

---

**Total:** ~51,600 LOC Rust | Edition 2021 | MSRV 1.80 | 191 unit tests | 17 crates

→ See [02-query-engine.md](./02-query-engine.md) for QueryEngine internals and tool dispatch.
