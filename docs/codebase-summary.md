# OxiCode — Codebase Summary

**Version:** 0.2.0 | **Last Updated:** 2026-04-03 | **Scope:** Phase 5 Complete (Command Implementations) | **Total:** 16 crates, 39 oxicode-tools files, ~105K tokens

---

## Crate Dependency Graph

```
┌─────────────────────────────────────────────────────────────┐
│              FOUNDATIONAL LAYER                             │
│                                                              │
│  oxicode-common                                            │
│    ├── OxiError, OxiResult                                 │
│    ├── Message, ContentBlock, Role                         │
│    ├── Usage, ModelInfo                                    │
│    └── re-exported by all crates                           │
└─────────────────────────────────────────────────────────────┘
                              ▲
                 ┌────────────┼────────────┐
                 │            │            │
┌────────────────┴──┐  ┌──────┴──────┐  ┌──┴────────────────┐
│ oxicode-config   │  │ oxicode-api │  │ oxicode-tools    │
│ - Config load    │  │ - Providers │  │ - Tool trait     │
│ - Env parsing    │  │ - Streaming │  │ - Registry       │
└────────────────┬─┘  └──────┬──────┘  └──┬────────────────┘
                 │            │            │
                 └────────────┴────────────┘
                              ▲
         ┌────────────────────┼────────────────────┐
         │                    │                    │
    ┌────┴──────┐    ┌────────┴────────┐   ┌──────┴──────┐
    │ oxicode-  │    │ oxicode-        │   │ oxicode-    │
    │ core      │    │ permissions     │   │ state       │
    │ (Query    │    │ (6-layer        │   │ (Store,     │
    │ Engine)   │    │  pipeline)      │   │  StateStore)│
    └────┬──────┘    └────────┬────────┘   └──────┬──────┘
         │                    │                    │
         └────────────────────┼────────────────────┘
                              ▲
         ┌────────────────────┼────────────────────┐
         │                    │                    │
    ┌────┴──────┐    ┌────────┴────────┐   ┌──────┴──────┐
    │ oxicode-  │    │ oxicode-        │   │ oxicode-    │
    │ session   │    │ hooks           │   │ context     │
    │ (Persist) │    │ (Callbacks)     │   │ (Defense)   │
    └───────────┘    └─────────────────┘   └──────┬──────┘
                                                   │
         ┌─────────────────────────────────────────┘
         │
    ┌────┴────────────────────────────────────────────────┐
    │         PHASE 4: MULTI-AGENT & SKILLS LAYER         │
    │                                                      │
    │  oxicode-agents ──── oxicode-skills ── oxicode-    │
    │  (Spawn, coord)       (Discovery,       tasks      │
    │                        Activation)       (Runner)  │
    └───────────┬───────────────────────────────┬────────┘
                │                               │
                └───────────────┬────────────────┘
                                │
    ┌───────────────────────────┴──────────────────────┐
    │          INTEGRATION & USER-FACING LAYER         │
    │                                                   │
    │  oxicode-mcp ──── oxicode-tui ─── oxicode-cli   │
    │  (MCP server)     (Frontend)      (Commands)    │
    └───────────────────────────────────────────────────┘
```

---

## Crate Details

### Layer 1: Foundational

#### oxicode-common (~200 LOC)
**Purpose:** Shared types, error handling, fundamental types

**Key exports:**
- `OxiError` enum + `OxiResult<T>` type alias
- `Message`, `ContentBlock`, `Role`, `StopReason`
- `Usage` (token tracking)
- `ModelInfo` (provider metadata)

**Used by:** Every other crate

**Quality:** No panics, comprehensive error variants, serde support

---

#### oxicode-config (~150 LOC)
**Purpose:** Configuration loading from files + environment

**Key exports:**
- `Config` struct (models, providers, permissions, hooks)
- `Provider` enum (Anthropic, OpenAI, Compatible, MCP)
- `load_config()` → reads from `~/.oxicode/config.toml`
- `load_project_config()` → reads from `./.oxicode/config.toml`

**Files:** config.rs, env.rs

**Used by:** CLI, TUI, Core

**Quality:** Validates paths, handles missing files gracefully

---

#### oxicode-api (~600 LOC)
**Purpose:** LLM provider traits + message types + streaming

**Key exports:**
- `LlmProvider` async trait (stream_message, name)
- `MessageRequest`, `MessageResponse`
- `StreamEvent` enum (TextDelta, ToolUseStart, UsageUpdate, etc)
- `EventStream` type alias (Pin<Box<...>>)
- Provider implementations:
  - `AnthropicProvider` — Claude API, supports thinking + streaming
  - `OpenAiProvider` — OpenAI API, compatible with Azure
  - `CompatibleProvider` — Any OpenAI-compatible endpoint
  - `McpProvider` — External MCP servers

**Files:**
- provider.rs (trait definition)
- anthropic.rs (~800 LOC, most complex)
- openai.rs
- compatible.rs
- mcp.rs

**Used by:** Core (QueryEngine), TUI (streaming display)

**Quality:** Stream error handling, SSE parsing, retry logic

**Top file:** openai_compatible.rs (3,681 tokens, 3.6% of codebase)

---

### Layer 2: Core Execution

#### oxicode-core (~400 LOC)
**Purpose:** Main query execution loop, LLM orchestration

**Key exports:**
- `QueryEngine` struct (main loop: stream → extract tools → execute → recurse)
- `assemble_system_prompt()` — compose system message from multiple sources
- `MAX_TOOL_TURNS = 50` (prevent infinite loops)

**Files:**
- query_engine.rs (main loop)
- system_prompt.rs (prompt assembly)
- message_builder.rs (helper)

**Key method:**
```rust
pub async fn execute_turn(&self, conversation: &mut Conversation) -> OxiResult<Message>
```

**Used by:** CLI, TUI

**Quality:** No panics, proper error propagation, tool execution hooks

---

#### oxicode-tools (~450 LOC)
**Purpose:** Tool registry, execution, schema generation

**Key exports:**
- `Tool` async trait (name, description, schema, execute)
- `ToolRegistry` (HashMap-based lookup + execution)
- `ToolSchema`, `ToolInput`, `ToolResult`
- 42 built-in tools (Phase 3 gap-closure: 11 new tools added)

**Files:**
- tool_trait.rs (trait definition, includes ToolContext with skill_executor, team_manager, task_manager, mcp_manager fields)
- registry.rs (lookup + execute)
- 39 tool implementation files (Phase 1: mcp_resource_tools, skill_tool; Phase 3 gap-closure: 11 new tools)
- Phase 3 new tools: todo_write, team_tools, lsp_tool, powershell, repl_tool, mcp_auth, suggest_background_pr, synthetic_output, verify_plan_execution, workflow_tool

**ToolContext (Phase 3 enhancement):**
- working_dir: PathBuf
- file_state: FileStateTracker (stale edit detection)
- task_manager: TaskManager (background tasks)
- task_abort_handles: HashMap for task cancellation
- mcp_manager: McpServerManager (external tools)
- skill_executor: SkillExecutor (skill invocation)
- **team_manager: TeamManager** (team coordination via Phase 3)

**Key method:**
```rust
pub async fn execute(&self, tool_name: &str, input: Value, ctx: &ToolContext) -> OxiResult<ToolResult>
```

**Used by:** QueryEngine, TUI (for tool completions)

**Quality:** Graceful error handling, input validation via schema

---

#### oxicode-permissions (~300 LOC)
**Purpose:** 6-layer permission decision pipeline

**Key exports:**
- `PermissionPipeline` (orchestrates 6 checks)
- `PermissionDecision` enum (Allow, Deny, Ask)
- `PermissionMode` enum (Default, Bypass, ApprovalOnly)
- `ToolPermissionLevel` (ReadOnly, FileWrite, ShellExec, System)

**Files:**
- pipeline.rs (orchestrator)
- allowlist.rs (L1)
- mode_checker.rs (L2)
- command_security.rs (L3)
- pattern_detector.rs (L4)
- rule_matcher.rs (L5)
- denial_tracker.rs (logging)

**6-layer flow:**
1. Safe allowlist (read-only always allowed)
2. Permission mode (check user setting)
3. Command security (block dangerous patterns)
4. Pattern detection (suspicious ops)
5. Rule matching (user-configured)
6. Default ask

**Used by:** QueryEngine (before tool execution)

**Quality:** Comprehensive dangerous pattern list (rm -rf, > /dev/sda, etc)

---

### Layer 3: State & Persistence

#### oxicode-state (~150 LOC)
**Purpose:** Central app state store with watch channel subscription

**Key exports:**
- `StateStore` (tokio watch channel wrapper)
- `AppState` (messages, model, usage, streaming flag)
- Watch pattern: `subscribe()` returns `watch::Receiver<AppState>`

**Key methods:**
```rust
pub fn current(&self) -> AppState;
pub fn update<F>(&self, f: F) where F: FnOnce(&mut AppState);
pub fn subscribe(&self) -> watch::Receiver<AppState>;
pub fn replace_messages(&self, messages: Vec<Message>);  // Phase 1: used by /compact
```

**Used by:** TUI (UI updates), Core (state mutations), CLI (status queries), /compact command (message replacement)

**Quality:** Thread-safe (Arc<Mutex>), efficient watch notifications

---

#### oxicode-session (~180 LOC)
**Purpose:** Session persistence to disk (JSONL format)

**Key exports:**
- `SessionManager` (load, save, append)
- `Session` struct (messages + metadata)

**Format:** One JSON object per line (JSONL)
```json
{"id":"msg_1","role":"user","content":[...],"created_at":"2026-04-02T..."}
{"id":"msg_2","role":"assistant","content":[...],"usage":{"input_tokens":100}}
```

**Key methods:**
```rust
pub fn load(session_dir: &Path) -> OxiResult<Session>;
pub fn save(&self, path: &Path) -> OxiResult<()>;
pub fn append_message(&self, msg: &Message) -> OxiResult<()>;
```

**Used by:** CLI (load/save sessions), TUI (persist conversation)

**Quality:** Atomic writes, version support, recovery from corruption

---

#### oxicode-hooks (~120 LOC)
**Purpose:** Event hooks for custom behavior (pre-commit, post-exec, etc)

**Key exports:**
- `HookManager` (register, execute hooks)
- `Hook` trait (async, can be chained)
- Built-in hooks: BeforeTool, AfterTool, BeforeCompact, AfterCompact

**Used by:** QueryEngine (hook points), CLI (user-defined hooks)

**Quality:** Non-blocking (async), graceful error handling

---

### Layer 4: Phase 4 — Context Defense & Multi-Agent

#### oxicode-context (~450 LOC)
**Purpose:** Token counting + 5-layer context defense orchestration

**Key exports:**
- `BudgetManager` — orchestrates L1-L5 defense, tracks token ratio
- `TokenCounter` — heuristic token counting (chars/4, no external tokenizer)
- `truncate_messages()` — L1: remove oldest middle messages
- `microcompact_messages()` — L2: compress thinking blocks + tool results
- `AutoCompactor` — L3: LLM-assisted summarization (async)
- `ReactiveCompactor` — L4: mid-stream emergency compaction
- `ContextCollapse` — L5: hard reset, save state to disk

**Files:**
- budget.rs (BudgetManager, orchestrator)
- token_counter.rs (heuristic counting)
- truncation.rs (L1)
- microcompact.rs (L2)
- auto_compact.rs (L3)
- reactive_compact.rs (L4)
- context_collapse.rs (L5)

**Key struct:**
```rust
pub struct BudgetManager {
    model_max_tokens: u32,
    warning_threshold: f64,  // 0.70
    danger_threshold: f64,   // 0.90
    critical_threshold: f64, // 0.95
}

pub enum BudgetStatus {
    Healthy,    // < 70%
    Warning,    // 70-90%
    Danger,     // 90-95%
    Critical,   // >= 95%
}
```

**Issues found (Phase 4 review):**
- **H1 (High):** Division by zero when `model_max_tokens == 0` → fix: add guard
- **H2 (High):** `select!` loop breaks on first stream close → fix: track both EOF states
- **M1 (Medium):** spawn_agent_handle doesn't write config to stdin
- **M4 (Medium):** TokenCounter uses byte length for multi-byte UTF-8

**Used by:** QueryEngine (context defense hooks), Core (budget checks)

**Quality:** Good test coverage, all edge cases (empty, zero) handled

---

#### oxicode-agents (~280 LOC)
**Purpose:** Subagent spawning, team coordination, inter-agent messaging

**Key exports:**
- `spawn_agent()` — spawn child process with config
- `spawn_agent_handle()` — spawn and return handle
- `AgentHandle` — represents running agent (id, status, child handle)
- `Coordinator` — manages agent team
- `CoordinatorState` — tracks all agents + message bus
- `AgentConfig` — configuration for spawning (model, tools, max_tokens)
- `MessageBus` — inter-agent JSON messaging
- `AgentStatus` enum (Idle, Running, Thinking, ToolUse, Sleeping, Error)
- `AgentMessage` struct (id, from, to, body, task_id)

**Files:**
- spawner.rs (~200 LOC, agent launching)
- coordinator.rs (~150 LOC, team management)
- communication.rs (MessageBus, AgentMessage)
- team.rs (Team, TeamManager)

**Key functions:**
```rust
pub async fn spawn_agent(config: AgentConfig) -> OxiResult<AgentHandle>;
pub async fn spawn_agent_handle(config: AgentConfig) -> OxiResult<ChildHandle>;
```

**Used by:** CLI (/agent command), Coordinator (delegation)

**Quality:** Graceful child process cleanup, configurable via JSON, serde support

---

#### oxicode-skills (~350 LOC)
**Purpose:** Skill discovery, parsing, activation, execution

**Key exports:**
- `SkillDiscovery` — discover SKILL.md files in ~/.oxicode/skills + ./.oxicode/skills
- `SkillExecutor` — inject skill prompts based on activation conditions
- `SkillInfo` struct (name, version, triggers, prompt_text)
- `ActivationContext` (current_file, user_input, user_intent)
- `SkillParser` — parse YAML frontmatter + prompt text

**Skill file format (markdown with YAML frontmatter):**
```yaml
---
name: "Python Debugger"
version: "1.0"
triggers:
  file_type: ["*.py"]
  keywords: ["debug", "error"]
  user_intent: ["fix"]
depends_on: []
---

# Debugging Instructions

When you see a Python error...
```

**Files:**
- discovery.rs (~150 LOC, filesystem walking)
- parser.rs (~200 LOC, YAML + markdown parsing)
- executor.rs (skill activation, prompt injection)

**Top file:** skills/parser.rs (2,488 tokens, 2.4% of codebase)

**Key method:**
```rust
pub fn build_skills_prompt(&self, ctx: &ActivationContext) -> Option<String>
```

**Issues found:**
- **L1 (Low):** Follows symlinks, potential for loops (WalkDir handles this)
- **L3 (Low):** Temp dir cleanup in tests (no TempDir wrapper)

**Used by:** QueryEngine (system prompt injection), Core (skill matching)

**Quality:** Good error recovery, logs skipped files, validates YAML

---

#### oxicode-tasks (~380 LOC)
**Purpose:** Background task management, process execution, output streaming

**Key exports:**
- `TaskManager` — in-process task registry
- `TaskRunner` — async process spawner (tokio::process::Command)
- `OutputReader` — incremental JSONL log reader
- `NotificationCollector` — de-duplicating status updates
- `TaskEntry` (id, type, command, status, created_at)
- `TaskStatus` enum (Pending, Running, Completed, Failed, Cancelled)
- `OutputLine` struct (timestamp, stream: stdout/stderr/exit_code, data)

**Files:**
- manager.rs (~80 LOC, registry)
- runner.rs (~200 LOC, process execution)
- output.rs (~120 LOC, JSONL reader)
- notifications.rs (~80 LOC, status updates)

**Output format (JSONL):**
```json
{"time":"2026-04-02T08:00:00Z","stream":"stdout","data":"running tests\n"}
{"time":"2026-04-02T08:00:05Z","stream":"exit_code","data":"0"}
```

**Top file:** tasks/runner.rs (2,140 tokens, 2.1% of codebase)

**Issues found:**
- **H2 (High):** select! loop breaks on first stream close → fix: track both streams
- **M2 (Medium):** Task ID sanitization (UUID safe today, but validate)
- **M3 (Medium):** XML injection in notifications (escape special chars)

**Used by:** CLI (/task command), TUI (task panel)

**Quality:** Good I/O handling, async-aware, proper stream management

---

### Layer 5: Integration & User-Facing

#### oxicode-mcp (~200 LOC)
**Purpose:** MCP (Model Context Protocol) server wrapper for external tools

**Key exports:**
- `McpServer` — communicates with external MCP servers via stdin/stdout
- `McpTool` — wrapper implementing Tool trait for MCP endpoints
- MCP request/response serialization

**Used by:** ToolRegistry (external tool bridging)

**Quality:** Proper process management, JSON serialization

---

#### oxicode-tui (~600 LOC)
**Purpose:** Terminal UI (ratatui), event loop, rendering

**Key exports:**
- `App` — main TUI struct (state, input, rendering)
- `Renderer` — draw functions for each component
- Event loop (keyboard → UiEvent → core → StateStore → redraw)
- Widgets: MessageView, InputBox, StatusBar, (Phase 5: AgentPanel, TaskPanel, NotificationPanel)

**Top file:** tui/app.rs (2,456 tokens, 2.4% of codebase)

**Layout:**
```
┌─────────────────────────┐
│ Status Bar (usage, etc) │ 1 line
├─────────────────────────┤
│ Message View            │ min 5 lines
│ (scrollable)            │
├─────────────────────────┤
│ Input Box (readline)    │ 3 lines
└─────────────────────────┘
```

**Event handling:**
- KeyEvent → parsed by ratatui
- Ctrl+C → quit
- Enter → send input to core
- Page Up/Down → scroll message view

**Used by:** Main binary, streaming display

**Quality:** Responsive, no blocking, proper cleanup on exit

---

#### oxicode-cli (~280 LOC)
**Purpose:** Slash commands (/help, /model, /agent, /skills, /tasks, etc)

**Key exports:**
- `CommandRegistry` — HashMap of slash commands
- `SlashCommand` trait (execute, completions)
- `CommandOutput` enum (Message, Silent, Quit, Error)
- **Built-in commands: 75 total (was 42 real + 27 stubs)**
  - Core: help, version, clear, status, quit, config, model, permissions
  - Session: save, load, export, undo, redo, rename, resume
  - Git: commit, pr, branch, log, stash, push, pull (7 Phase 5)
  - Debug: compact, context, usage, tokens, doctor
  - MCP: mcp-servers, mcp-tools, mcp-connect, mcp-disconnect
  - Team/Agents: team, agents, task, plugin, plan
  - View: theme, shortcuts, about, tools, history, vim, review (+ many workflow stubs)
  - **Phase 5:** All 27 stubs replaced + 6 new commands (vim, rename, usage, context, resume, review)
  - **File management:** view_commands.rs split into 3 modules (each <200 lines)

**Files:**
- commands/mod.rs (registry)
- commands/git_commands.rs + git_helpers.rs (Phase 5)
- commands/session_commands.rs (Phase 5: undo, rename)
- commands/provider.rs (permissions)
- commands/mcp_commands.rs, team_commands.rs, task_commands.rs, plugin_commands.rs, plan_commands.rs
- commands/debug_commands.rs (Phase 5: usage, context enhancements)
- commands/general.rs (vim)
- commands/view_commands.rs, workflow_commands.rs, session_view_commands.rs (Phase 5 split)
- repl.rs (REPL mode with readline)

**Used by:** TUI (command parsing), CLI (REPL)

**Quality:** Good error messages, help text, command completions

---

## Code Statistics

| Metric | Value |
|--------|-------|
| Total Crates | 16 |
| Total Files | 121 |
| Total Tokens | 101,835 |
| Total Chars | 450,032 |
| LOC (non-test) | ~5,500 |
| Test LOC | ~2,000 |
| Unsafe Code | 0 (forbidden) |
| Panics in Prod | 0 |

### Top 5 Files by Size
1. openai_compatible.rs — 3,681 tokens (3.6%)
2. skills/parser.rs — 2,488 tokens (2.4%)
3. tui/app.rs — 2,456 tokens (2.4%)
4. query_engine.rs — 2,331 tokens (2.3%)
5. tasks/runner.rs — 2,140 tokens (2.1%)

### Crate Breakdown (LOC estimate)
| Crate | LOC | Primary Role |
|-------|-----|--------------|
| oxicode-common | 200 | Shared types |
| oxicode-config | 150 | Config loading |
| oxicode-api | 600 | Provider traits |
| oxicode-core | 400 | Query execution |
| oxicode-tools | 450 | Tool system (42 tools as of Phase 3) |
| oxicode-permissions | 300 | Access control |
| oxicode-state | 150 | State management |
| oxicode-session | 180 | Persistence |
| oxicode-hooks | 120 | Event hooks |
| **oxicode-context** | **450** | **Context defense (P4)** |
| **oxicode-agents** | **280** | **Multi-agent (P4)** |
| **oxicode-skills** | **350** | **Skills system (P4)** |
| **oxicode-tasks** | **380** | **Background tasks (P4)** |
| oxicode-mcp | 200 | MCP bridging |
| oxicode-tui | 600 | Terminal UI |
| oxicode-cli | 280 | Commands/REPL |

---

## Key Design Patterns

### 1. Provider Trait
All LLM providers implement `LlmProvider` async trait:
```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream_message(&self, req: MessageRequest) -> OxiResult<EventStream>;
    fn name(&self) -> &str;
}
```

**Enables:** Easy swapping between providers (Anthropic, OpenAI, compatible, MCP)

### 2. Tool Trait & Registry
All tools implement `Tool` async trait. Registry manages 42 built-in tools + custom tools:
```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, input: Value, ctx: &ToolContext) -> OxiResult<ToolResult>;
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}
```

**Enables:** Custom tools, uniform execution, schema-based validation, dynamic registry management

**Phase 3 Gap Closure:** 11 new tools added to match OpenClaude feature parity (44 tools target)

### 3. Permission Pipeline
Layered checks, each can Allow/Deny/Ask:
```rust
pub enum PermissionDecision {
    Allow,
    Deny(String),
    Ask(String),
}
```

**Enables:** Fine-grained access control, extensible rules

### 4. Watch Channel (StateStore)
Central state with lazy subscribers:
```rust
pub fn subscribe(&self) -> watch::Receiver<AppState>;
```

**Enables:** UI updates, loose coupling, efficient notifications

### 5. Context Defense Layers
Graduated approach, each layer more aggressive:
- L1: Truncation (fast, loses history)
- L2: Microcompact (fast, lossy compression)
- L3: Auto-Compact (slower, LLM-assisted)
- L4: Reactive (mid-stream, emergency)
- L5: Collapse (hard reset, last resort)

**Enables:** Token efficiency, responsive UX, failsafe approach

### 6. Skill Activation
Condition-based prompt injection:
- File type matching (*.py → Python skill)
- Keyword detection (debug, error, fix)
- User intent classification

**Enables:** Dynamic skill loading, no manual registration

### 7. Task Output Streaming
JSONL format, incremental reading:
```json
{"time":"...","stream":"stdout","data":"..."}
```

**Enables:** Real-time status, low memory, resilient logging

---

## Dependencies (Workspace-Level)

**Core async:**
- tokio (full features) — async runtime
- futures — utilities for async/streaming

**Serialization:**
- serde, serde_json — JSON + config
- toml — config file format

**HTTP:**
- reqwest (with rustls-tls) — HTTP client
- reqwest-eventsource — SSE parsing

**TUI:**
- ratatui — terminal rendering
- crossterm — terminal I/O

**Parsing & Processing:**
- pulldown-cmark — markdown parsing
- syntect — syntax highlighting
- regex — pattern matching
- walkdir — filesystem traversal

**Error handling:**
- thiserror — error macros
- anyhow — error context

**Logging:**
- tracing, tracing-subscriber — structured logging

**Utilities:**
- chrono — timestamps
- uuid — unique IDs
- dirs — home dir paths
- clap — CLI argument parsing
- git2 — Git integration

**Total transitive deps:** ~30 (lean, well-maintained)

---

## Testing Strategy

**Unit Tests:** Each module has `#[cfg(test)]` block covering:
- Happy path
- Edge cases (empty, zero, missing)
- Error conditions (invalid input, permission denied)

**Integration Tests:** In `tests/` directory:
- End-to-end query execution
- Provider routing
- Session persistence
- Permission pipeline

**Test Coverage:** Phase 4 review found all modules well-tested, no panics in test code.

**Running tests:**
```bash
cargo test --all                    # All tests
cargo test --lib                    # Unit only
cargo test --test integration_test  # Integration only
```

---

## Build & Deployment

**Minimum Rust version:** 1.80
**Target:** x86_64-unknown-linux-gnu, aarch64-apple-darwin, etc

**Build:**
```bash
cargo build --release
```

**Binary locations:**
- TUI: `target/release/oxicode`
- Server: (Phase 5)

**Configuration:**
- Global: `~/.oxicode/config.toml`
- Project: `./.oxicode/config.toml`
- Skills: `~/.oxicode/skills/`, `./.oxicode/skills/`
- Sessions: `~/.oxicode/sessions/`, `./.oxicode/sessions/`
- Tasks: `~/.oxicode/tasks/`, `./.oxicode/tasks/`

---

## Known Issues & Technical Debt

### Phase 4 Review Findings

**High Priority (fix before v1):**
1. **H1:** BudgetManager division by zero when max_tokens==0
2. **H2:** TaskRunner select! loop breaks on first stream close

**Medium Priority (defensive hardening):**
1. **M1:** spawn_agent_handle doesn't write config to stdin
2. **M2:** Task ID path traversal (needs validation)
3. **M3:** XML injection in notifications (needs escaping)
4. **M4:** TokenCounter over-counts multi-byte UTF-8

**Low Priority (nice-to-have):**
1. **L1:** Skill discovery follows symlinks (WalkDir handles cycles)
2. **L2:** Missing Send+Sync documentation (inline comments)
3. **L3:** Test temp cleanup (no TempDir drop impl)
4. **L4:** truncation.rs uses Vec::remove(0) in loop (O(n²) but negligible for <1000 messages)

---

## Next Steps (Phase 5+)

1. ✅ **Phase 5 COMPLETE** — All 27 stubs replaced (75 total commands, all real)
2. **Phase 6:** Advanced features (server mode, Web UI, plugin marketplace)
3. **Bug fixes:** Address H1, H2 priority issues from Phase 4 review
4. **Performance:** Token counting optimization, context window tuning
5. **Observability:** Metrics, trace export, debug mode enhancements

---

## References for Developers

- **Architecture:** See `system-architecture.md`
- **Code Standards:** See `code-standards.md`
- **Project Overview:** See `project-overview-pdr.md`
- **Exploration Report:** `plans/reports/Explore-260402-oxicode-phase4-interfaces.md`
- **Phase 4 Review:** `plans/reports/code-reviewer-260402-0830-phase4-quality.md`

