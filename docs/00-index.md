# OxiCode Design Documents

Navigation hub for all OxiCode design documentation. Start here to understand the system architecture, then dive into specific subsystems.

## Reading Order

1. **00-index.md** (this file) — Overview and crate map
2. **01-architecture.md** — System architecture, startup, request lifecycle, and dependency DAG
3. **02-query-engine.md** — Multi-turn loop, tool dispatch, budget management, and permissions
4. **03-tool-system.md** — Tool trait, registry, 49 built-in tools, MCP integration
5. **04-permission-system.md** — 6-layer permission system, allow/deny/ask decision model
6. **05-llm-providers.md** — LLM provider trait, implementations (Anthropic, OpenAI, Bedrock, Vertex)
7. **06-tui.md** — Terminal UI, event loops, streaming markdown, themes
8. **07-subsystems.md** — Session persistence, StateStore, hooks, agents, skills
9. **08-code-standards.md** — Toolchain, error handling, testing, performance optimization

**Operations & Deployment:**

- **operations.md** — OTLP telemetry setup (Jaeger, Grafana) and bridge mode quick start
- **bridge-mode.md** — Bridge mode operator guide: JWT auth, WebSocket protocol, env vars, security

## Crate Map — 17-Crate Workspace

```
┌─────────────────────────────────────────────────────────────┐
│ LAYER 4 — BINARY ENTRY POINT                                │
├─────────────────────────────────────────────────────────────┤
│                      oxicode-cli                            │
│                                                              │
│  Entry point: main.rs                                       │
│  Modes: TUI, single-prompt, MCP server, daemon, agent       │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────┬──────────────────┬──────────────────────┐
│ LAYER 3 — ENGINE │     ENGINE       │   PLUGINS & TEAMS   │
├──────────────────┼──────────────────┼──────────────────────┤
│ oxicode-core     │  oxicode-tui     │  oxicode-agents     │
│ (QueryEngine)    │  (ratatui TUI)   │  (team manager)     │
│                  │                  │                      │
│ oxicode-skills   │                  │  oxicode-plugins    │
│ (skill executor) │                  │  (subprocess tools) │
└──────────────────┴──────────────────┴──────────────────────┘
           ↓              ↓                    ↓
┌────────────────────────────────────────────────────────────┐
│ LAYER 2 — SERVICES                                         │
├────────────────────────────────────────────────────────────┤
│ oxicode-api          → LlmProvider trait + implementations │
│ oxicode-tools        → Tool trait + ToolRegistry (49 tools)│
│ oxicode-permissions  → PermissionPipeline (6-layer)       │
│ oxicode-mcp          → MCP client (stdio, SSE, HTTP, WS)   │
│ oxicode-context      → BudgetManager, token counting       │
│ oxicode-tasks        → Background task management          │
│ oxicode-hooks        → 26 lifecycle hook events            │
└────────────────────────────────────────────────────────────┘
                              ↓
┌────────────────────────────────────────────────────────────┐
│ LAYER 1 — FOUNDATION                                       │
├────────────────────────────────────────────────────────────┤
│ oxicode-config   → TOML + env + CLAUDE.md loading         │
│ oxicode-session  → Session persistence (save/load/resume) │
│ oxicode-state    → StateStore + AppState (watch channels) │
│ oxicode-common   → Message, ContentBlock, Role, OxiError  │
└────────────────────────────────────────────────────────────┘
```

## Key Traits Quick Reference

### LlmProvider (oxicode-api)

**Purpose:** Abstract interface for LLM backends.

```rust
pub trait LlmProvider: Send + Sync {
    async fn stream_message(
        &self, 
        request: MessageRequest
    ) -> OxiResult<EventStream>;
    
    fn name(&self) -> &str;
}
```

**Implementations:** Anthropic (primary), OpenAI-compatible, AWS Bedrock, Google Vertex AI.

**StreamEvent variants:** `TextDelta(String)`, `ToolUseStart { id, name, input }`, `ToolUseEnd { id }`, `UsageUpdate { input, output }`, `ErrorEvent(OxiError)`.

### Tool (oxicode-tools)

**Purpose:** Any action the LLM can request — file I/O, bash, MCP, etc.

```rust
pub trait Tool: Send + Sync {
    async fn execute(
        &self, 
        input: Value, 
        ctx: &ToolContext
    ) -> OxiResult<ToolResult>;
}

pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}
```

**Built-in tools (49):** `bash`, `file_read`, `file_write`, `file_delete`, `bash_long_running`, `grep`, `gh_pr_view`, `web_fetch`, `mcp_*` (proxy to MCP servers).

### PermissionPipeline (oxicode-permissions)

**Purpose:** 6-layer allow/deny/ask decision model for tool execution.

```rust
pub enum PermissionDecision {
    Allow,
    Deny(String),
    Ask,  // TUI shows dialog, user decides
}
```

**Layers (evaluated in order):**
1. Safe allowlist (always allow: `file_read`, `bash` on safe commands)
2. Command security hard-deny (block dangerous patterns: `rm -rf`, `dd if=`)
3. Mode bypass (if permission_mode = `allow_all`, skip to Allow)
4. Dangerous pattern detection (flag risky combinations)
5. User-defined rules (e.g., whitelist/blacklist patterns)
6. Default decision (Ask, Deny, or Allow based on mode)

**Modes:** `ask` (default), `allow_all`, `deny_all`, `safe_only`.

### QueryEngine (oxicode-core)

**Purpose:** Main orchestration loop for multi-turn conversations with tool support.

```rust
pub struct QueryEngine {
    provider: Arc<dyn LlmProvider>,
    state_store: Arc<StateStore>,
    tool_registry: Arc<ToolRegistry>,
    permission_pipeline: Arc<PermissionPipeline>,
    tool_context: ToolContext,
    model: StdMutex<String>,
    max_tokens: u32,
    system_prompt: String,
    budget_manager: Mutex<BudgetManager>,
}

pub async fn execute_turn(
    &self,
    conversation: &mut Conversation,
    event_tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>,
) -> OxiResult<Message>
```

**Flow:**
1. Get user input → build `Conversation` from message history
2. Call `LlmProvider::stream_message()` with `MessageRequest`
3. Stream events back (text, tool_use_start, tool_use_end)
4. Extract tool calls → permission check → execute via tool registry
5. Append tool results → loop until `EndTurn` or `MaxTokens`

**Max tool turns:** 50 (safety limit).

### StateStore (oxicode-state)

**Purpose:** Centralized app state (messages, model, auth info) shared via `tokio::sync::watch`.

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

**Subscribers:** TUI, agent mode, session persistence.

**Pattern:** All state mutations go through `state_store.update(|s| { s.field = value; })` or `push_message()`.

## Glossary

| Term | Meaning |
|------|---------|
| **Turn** | One round of LLM → response → tool execution → result appended |
| **Compaction** | Automatic context window defense: summarize old messages into single summary message when context exceeds budget |
| **Budget** | Context window size limit (default 200k tokens); tracks used tokens and triggers compaction |
| **Stream Event** | One discrete event from LLM (text chunk, tool call, end signal); consumed by TUI for live rendering |
| **Tool Dispatch** | Process of extracting tool calls from LLM response, permission checking, and executing via tool registry |
| **Permission Decision** | Result of PermissionPipeline: `Allow`, `Deny(reason)`, or `Ask` |
| **Cancel Flag** | Shared `Arc<AtomicBool>` set by TUI on Ctrl+C; engine checks between events to abort turn |
| **CoreEvent** | Event from engine to TUI (`TextDelta`, `ToolUseStart`, `PermissionAsk`, error, etc.) |
| **UiEvent** | Event from TUI to engine (`UserInput`, `SlashCommand`, `Quit`, etc.) |
| **TurnEvent** | Internal event within QueryEngine (same as CoreEvent but used in engine task) |
| **Content Block** | One unit of message content (`Text`, `Image`, `ToolUse`, `ToolResult`, `Thinking`) |
| **Thinking Block** | Extended thinking output (Claude's chain-of-thought); not sent to LLM API |
| **Session** | Persistent conversation (ID, messages, created_at) saved to disk; resumable via `--session <id>` |
| **Memory** | Project-level context loaded from `~/.oxicode/projects/{key}/memory/MEMORY.md` (+ extra .md files); injected into system prompt at startup, capped at 100 KB |
| **MCP** | Model Context Protocol — standardized way to define tools; OxiCode is an MCP client |
| **Hook** | Lifecycle event hook (26 total) — fire at key moments (session_start, turn_complete, etc.) for extensibility |
| **Agent** | Subagent spawned by main OxiCode, receives config via stdin, outputs result to stdout |
| **Skill** | Python/shell script discovered from `~/.oxicode/skills/` or `.oxicode/skills/`; injected into system prompt |

---

**Source:** OxiCode workspace, 17 crates, ~51k LOC Rust, Edition 2021, MSRV 1.80.

→ See [01-architecture.md](./01-architecture.md) for full system architecture.
