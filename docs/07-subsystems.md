# OxiCode Subsystems Architecture

17 crates, ~200k LOC, 29 hook events, 49 built-in tools, 6-layer permission pipeline, 3-level config merge, subprocess isolation, NDJSON streaming.

**Request flow:** User input → `QueryEngine` (oxicode-core) → `LlmProvider` (oxicode-api) → stream response → `ToolRegistry` dispatches tool calls → `PermissionPipeline` gates execution → results feed back to LLM.

---

## 1. MCP Client (`oxicode-mcp`)

Manages connections to external model context protocol (MCP) servers via stdin, HTTP, SSE, or WebSocket transports.

### Key Types

- **`McpToolDef`** — Re-export of `rmcp::model::Tool`; describes name, description, input JSON schema
- **`McpToolResult`** — Re-export of `rmcp::model::CallToolResult`; result/error from tool execution
- **`McpResource`** — Re-export of `rmcp::model::Resource`; file or data resource reference
- **`McpServerManager`** — Central coordinator; owns HashMap<String, ManagedClient>

### Transport Types

```
TokioChildProcess    ← Stdio (local subprocess)
StreamableHttpClientTransport ← HTTP / SSE (remote)
WebSocket (future)
```

### Lifecycle Flow

1. **Init phase**: `start_from_config()` → iterate enabled servers
2. **Per-server startup**: Create transport based on config (command/args/env for stdio; URL for HTTP)
3. **Discovery**: `list_all_tools()` via rmcp protocol → cache Tool list
4. **Registration**: Tools prefixed as `server__toolname` (e.g., `filesystem__read_file`)
5. **Execution**: `call_tool(server_name, tool_name, args)` → CallToolResult
6. **Shutdown**: Graceful disconnect on session end

### Tool Prefixing Convention

All MCP tools use the `server__toolname` prefix to avoid collisions with built-in tools. Tool schemas are converted to Claude-format via `mcp_tool_to_schema()` which adds `[MCP:servername]` to description.

### Error Handling

- Server startup failure → logged, non-fatal
- Tool discovery failure → logged, tools not added to registry
- Tool execution failure → returned as CallToolResult with error flag

**Source:** `crates/oxicode-mcp/src/manager.rs`, `types.rs`, `config.rs`

---

## 2. Hooks System (`oxicode-hooks`)

29 lifecycle hook events fire at key decision points. Hook scripts (agents or HTTP) receive `HookPayload` JSON, return `HookResponse` to allow Pass/ModifyPrompt/OverrideResult/Abort.

### Hook Event Categories

**Core (10):**
- `SessionStart` — after session initialization
- `SessionEnd` — before session exit
- `PreQuery` — before sending query to LLM
- `PostSampling` — after receiving LLM response
- `ToolCallBefore` — before tool execution
- `ToolCallAfter` — after tool execution completes
- `PermissionRequest` — before permission dialog
- `ContextCompact` — before context compaction
- `ModelSwitch` — when active model changes
- `Error` — on error occurrence

**Extended (16):**
- `CompactComplete`, `AgentSpawn`, `AgentComplete`, `SkillActivate`, `PluginInit`, `PluginShutdown`, `SessionSave`, `SessionLoad`, `ThemeChange`, `CommandExecute`, `FileRead`, `FileWrite`, `FileEdit`, `BashExecute`, `PermissionGrant`, `PermissionDeny`

**Rate Limit (2):**
- `RateLimitWarning` — approaching threshold
- `RateLimitExceeded` — 429 received

**Cost (1):**
- `CostUpdate` — after cost tracker updated

### Hook Payload & Response

```rust
pub struct HookPayload {
    pub event: HookEvent,
    pub data: serde_json::Value,          // Event-specific data
    pub session_id: Option<String>,
    pub model: Option<String>,
}

pub enum HookResponse {
    Pass,                                  // Continue normally
    ModifyPrompt { text: String },         // Inject into system prompt
    OverrideResult { text: String },       // Replace tool result
    Abort { reason: String },              // Cancel operation
}
```

### Configuration & Execution

Config file: `~/.oxicode/hooks.yaml` maps events to handlers:
```yaml
hooks:
  pre_query:
    executor: agent | http
    command: /path/to/script  # for agent executor
    url: http://localhost:8888  # for HTTP executor
    timeout_secs: 5
```

**Executors:**
- **Agent executor**: Spawns subprocess, passes HookPayload on stdin, reads HookResponse from stdout
- **HTTP executor**: POSTs HookPayload to URL with 5s timeout

**Error handling:** Failed hook logged non-fatal; execution continues.

**Source:** `crates/oxicode-hooks/src/events.rs`, `manager.rs`, `executor.rs`

---

## 3. Configuration (`oxicode-config`)

3-level merge: code defaults → env vars → `~/.oxicode/settings.toml` → `CLAUDE.md`/`OXICODE.md` (project-level) → CLI flags.

### Settings Struct

```rust
pub struct Settings {
    pub config_version: u32,               // Migration tracking
    pub api_key: Option<String>,           // From ANTHROPIC_API_KEY env
    pub model: String,                     // e.g., "claude-sonnet-4-20250514"
    pub max_tokens: u32,
    pub theme: String,                     // TUI theme name
    pub permission_mode: String,           // "default" | "accept_edits" | "bypass"
    pub config_dir: Option<String>,
    pub features: FeatureFlags,
    pub editor_mode: String,               // "normal" | "vim"
    pub output_style: String,              // "plain" | "markdown" | "minimal" | "verbose"
    pub default_haiku_model: Option<String>,
    pub default_sonnet_model: Option<String>,
    pub default_opus_model: Option<String>,
}
```

### Precedence Order (lowest → highest)

1. **Code defaults** (Settings::default())
2. **Env vars** (ANTHROPIC_API_KEY, OXICODE_MODEL, OXICODE_MAX_TOKENS, etc.)
3. **~/.oxicode/settings.toml** (user global config)
4. **CLAUDE.md / OXICODE.md** (project-level, read from cwd + parent dirs)
5. **CLI flags** (--model, --config-dir, etc.)

### Feature Flags

```rust
pub struct FeatureFlags {
    pub extended_thinking: bool,
    pub prompt_caching: bool,
    pub proactive_agents: bool,
    pub vim_mode: bool,
    pub voice: bool,               // Feature-gated (requires voice feature)
}
```

Toggle at runtime: `/feature toggle vim_mode`

### Migrations

Auto-applied on config load with backup. Examples:
- Old model name → new version
- Deprecated setting → new location
- Removed feature → graceful disable

**Source:** `crates/oxicode-config/src/settings.rs`, `lib.rs`, `migrations.rs`

---

## 4. Session Management (`oxicode-session`)

Persists conversations to disk for resumption. Storage at `~/.oxicode/sessions/{uuid}.json` (permissions 0o600).

### Session Lifecycle

1. **new()** → UUID + timestamps
2. **save()** → JSON to disk (atomic write)
3. **load()** from disk
4. **list_sessions()** → sorted by updated_at DESC
5. **resume()** via `oxicode --session <uuid>`

### Memory Subsystem

5 memory types classify importance:

```rust
pub enum MemoryType {
    Decision,                   // Architecture/design decisions
    Context,                    // Project facts/background
    Preference,                 // User style/rules
    Task,                       // Active todos
    Reference,                  // Links/docs
}
```

**Metadata per memory file:**
- filename, filepath, memory_type, description, tags, mtime

**Memory flow:**
1. **Extractor** → scans knowledge base, parses YAML frontmatter
2. **Scanner** → periodically finds new/changed memory files
3. **Selector** → picks top-N semantically similar chunks for current query
4. **Freshness** → temporal decay; recent memories weighted higher
5. **Injector** → prepended to system prompt before LLM query

**Storage:** `~/.oxicode/memory/` (markdown files with YAML frontmatter)

### Prompt History

Persisted to `~/.oxicode/history.json` (NDJSON format):
- Loaded on TUI start
- Accessed via Ctrl+R (reverse search)
- One entry per user query

### Team Memory Sync (Feature-gated)

- Upload memory to shared backend for team collaboration
- Pull teammate memories into local cache
- Requires `teammate` feature flag

**Source:** `crates/oxicode-session/src/lib.rs`, `memory*.rs`, `prompt_history.rs`, `team_memory_sync.rs`

---

## 5. State Management (`oxicode-state`)

Centralized `AppState` broadcast via `tokio::sync::watch` channels.

### AppState Fields

```rust
pub struct AppState {
    pub session_id: String,
    pub messages: Vec<Message>,           // Conversation history
    pub is_streaming: bool,
    pub current_model: String,
    pub total_usage: Usage,               // Token counts (input/output)
    pub active_agents: Vec<AgentEntry>,   // Subagents (for coordinator)
    pub active_skills: Vec<String>,       // Skill names
    pub background_tasks: Vec<TaskEntry>, // Background tasks
    pub feature_flags: FeatureFlags,
    pub last_rate_limit: Option<RateLimitSnapshot>,
    pub auth_label: String,               // e.g., "⚡ user@example.com"
    pub session_ingress_token: Option<String>,  // Bridge routing token
    pub cost_tracker: CostTracker,        // Per-model costs
    pub context_window_max: u32,          // Model's max tokens (0 = unknown)
    pub permission_mode: String,          // "ask" | "auto" | "bypass"
    pub cwd: String,                      // Current working directory
}
```

### StateStore API

```rust
pub struct StateStore {
    tx: watch::Sender<AppState>,
    rx: watch::Receiver<AppState>,
}

impl StateStore {
    pub fn new(initial: AppState) -> Self { ... }
    pub fn subscribe(&self) -> watch::Receiver<AppState> { ... }
    pub fn current(&self) -> AppState { ... }
    pub fn update<F>(&self, f: F) where F: FnOnce(&mut AppState) { ... }
    pub fn push_message(&self, message: Message) { ... }
    pub fn set_streaming(&self, streaming: bool) { ... }
    pub fn add_usage(&self, usage: Usage) { ... }
    pub fn set_active_skills(&self, skills: Vec<String>) { ... }
    pub fn update_agents(&self, agents: Vec<AgentEntry>) { ... }
    pub fn update_tasks(&self, tasks: Vec<TaskEntry>) { ... }
    pub fn toggle_feature(&self, flag: &str, enabled: bool) { ... }
}
```

### Cost Tracking

- `CostTracker` — HashMap<model_name, cost_in_cents>
- Persisted to disk (`~/.oxicode/cost_history.json`)
- Updated after each API call via `add_usage()`
- Displayed in `/status` command

**Broadcast Pattern:** `update()` → `send_modify()` → all subscribers receive new state. Used by TUI, CLI, agents for real-time sync.

**Source:** `crates/oxicode-state/src/lib.rs`, `cost_tracker.rs`

---

## 6. Multi-Agent System (`oxicode-agents`)

Spawns Research, Debugger, Reviewer, Tester agents; provides inter-agent messaging.

### Agent Types

```rust
pub enum AgentType {
    Research,      // Investigate topics, read docs
    Debugger,      // Analyze errors, propose fixes
    Reviewer,      // Code review, standards check
    Tester,        // Run tests, report coverage
}
```

### Spawning & Execution

1. **Fork mode** (`run_fork_agent()`): Subprocess spawned from same binary
   - Isolated context: working dir, env vars, file access
   - Task description via stdin JSON config
   - Status/output via watch channel

2. **In-process task** (optional): Agent logic runs as tokio task
   - Faster startup
   - Shared memory context

3. **Configuration** (`AgentConfig`):
   - agent_type, prompt, model, timeout
   - isolation: cwd, env overrides, file_restrictions
   - task_definition: text prompt

### Coordinator Mode

Routes tool calls to best agent type:

```rust
pub const COORDINATOR_TOOLS: &[&str] = &[
    "plan", "research", "review", "debug", "test",
    "commit", "execute", "fetch_docs", "summarize"
];

pub fn is_coordinator_tool(name: &str) -> bool { ... }
```

**Coordinator logic:**
- Intercepts `/plan`, `/research`, `/debug`, `/test` etc.
- Spawns corresponding agent
- Waits for agent completion
- Returns agent result to user

**Coordinator state tracks:**
- Active agents (name, status, started_at)
- Agent message queue

### Inter-Agent Communication

`MessageBus` — pub/sub for agents:
- `publish(agent_name, message)` → broadcasts to subscribers
- `subscribe(agent_name)` → receives messages from that agent
- Reduces redundant work (e.g., one agent's research shared with reviewer)

### Team Management

`TeamManager` + `TeamMember` for multi-session collaboration:
- Shared context + memory across sessions
- File ownership tracking
- Message passing protocol

**Source:** `crates/oxicode-agents/src/spawner.rs`, `coordinator.rs`, `communication.rs`, `team.rs`

---

## 7. Skills System (`oxicode-skills`)

Markdown files with YAML frontmatter that inject context into the conversation.

### Skill Format (SKILL.md)

```markdown
---
name: sequential-thinking
description: Prompt LLM to think step-by-step
activation:
  keywords: [think, reason, debug]
  paths: ["*.rs", "*.py"]
inject: system
---
Let's break this down step by step...
```

### Types

```rust
pub struct Skill {
    pub metadata: SkillMetadata,
    pub prompt: String,                   // Body after frontmatter
    pub source_path: PathBuf,
}

pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub activation: ActivationRule,
    pub inject: InjectMode,               // system | user
}

pub struct ActivationRule {
    pub paths: Vec<String>,               // Glob patterns
    pub keywords: Vec<String>,            // Case-insensitive keywords
}
```

### Activation Rules

Skills activate automatically when:
1. **Always** — no activation rule (injected every query)
2. **On user input** — any keyword (case-insensitive) matches user message
3. **On file type** — glob pattern matches current file path
4. **On feature flag** — feature_flag toggles (if configured)

### Discovery & Loading

1. **Bundled skills** — compiled into binary (sequential-thinking, code-reviewer, debugger, researcher, docs-seeker)
2. **User skills** — `~/.oxicode/skills/*.md`
3. **Project skills** — `./.claude/skills/*.md` (project-level)

Parser:
- Splits on `---` delimiters
- Parses YAML frontmatter manually (no external YAML crate)
- Body = prompt text

### Injection

**System mode:** Prepended to system prompt (affects model reasoning)
**User mode:** Injected as user message in turn context

Executor:
- Matches activation rules
- Selects applicable skills
- Concatenates prompts (in order)
- Injects via mode (system_prompt | turn_context)

**Source:** `crates/oxicode-skills/src/discovery.rs`, `parser.rs`, `executor.rs`, `bundled.rs`

---

## 8. Plugins System (`oxicode-plugins`)

Subprocess-isolated plugins via `plugin.toml` manifest.

### Plugin Manifest

```toml
[plugin]
name = "my-plugin"
version = "0.1.0"
description = "Does something"
author = "user"

command = "python3"
args = ["-m", "my_plugin"]
env = { RUST_LOG = "debug" }

[[tools]]
name = "my_tool"
description = "Does X"
input_schema = { type = "object", properties = { ... } }

[[commands]]
name = "my_command"
description = "User-facing command"

[[hooks]]
event = "session_start"
priority = 100

[lifecycle]
init = "init.sh"
shutdown = "shutdown.sh"
```

### Plugin Lifecycle

1. **Discovery** → scan `~/.oxicode/plugins/*/plugin.toml`
2. **Parse manifest** → PluginManifest struct
3. **Spawn subprocess** → tokio::process::Command with args/env
4. **Tool discovery** → JSON-RPC list_tools call
5. **Registration** → tools prefixed `plugin__toolname`
6. **Hook subscription** → register for events
7. **Shutdown** → SIGTERM, then SIGKILL after timeout

### Tool Registration

Tools from plugins registered with `plugin__` prefix (e.g., `plugin__analyze_code`). Input schemas validated at startup.

### Security Model

```rust
pub enum TrustLevel {
    Verified,      // Signed by maintainer
    Unverified,    // Unsigned, user confirmed
    Sandboxed,     // Strict capability limits
}

pub struct PluginCapability {
    file_read: bool,
    file_write: bool,
    bash_exec: bool,
    network: bool,
}
```

**Subprocess isolation:**
- Separate process
- Can limit via OS (seccomp, pledge, pledge)
- Environment variables scoped
- Stdin/stdout for JSON-RPC communication

### Error Handling

- Manifest parse error → plugin disabled
- Subprocess spawn error → logged, non-fatal
- Tool execution failure → returned as ToolResult with error
- Subprocess crash → auto-restart on next call (configurable)

**Source:** `crates/oxicode-plugins/src/manager.rs`, `manifest.rs`, `lifecycle.rs`, `security.rs`, `subprocess.rs`

---

## 9. Tasks System (`oxicode-tasks`)

Background task management with disk-based output streaming.

### Task Types

```rust
pub enum TaskType {
    LocalBash { command: String },
    LocalAgent { prompt: String, model: String },
    Monitor { interval_secs: u64, command: String },
    #[cfg(feature = "remote")]
    RemoteAgent { server_url: String, prompt: String, model: String },
    #[cfg(feature = "teammate")]
    InProcessTeammate { name: String, prompt: String, owned_files: Vec<String> },
    #[cfg(feature = "dream")]
    Dream { prompt: String, model: String, wake_interval_secs: u64 },
}

pub enum TaskStatus {
    Pending,
    Running,
    Completed { exit_code: i32 },
    Failed { error: String },
    Killed,
}
```

### Task Lifecycle

1. **Create** → `create_task(task_type)` → UUID + Pending status
2. **Run** → `run_task(id)` → status→Running
3. **Stream output** → JSONL to `~/.oxicode/tasks/{id}.jsonl`
4. **Notify** → task started, completed, failed, long-running (>30s idle)
5. **Read output** → `output_reader(id)` → stream JSONL lines
6. **Kill** → `kill_task(id)` → SIGTERM → SIGKILL

### Output Streaming

- **No memory buffering** — output written directly to disk as JSONL
- One JSON object per line: `{ "type": "stdout|stderr|status", "data": "..." }`
- Clients read via tail-like iterator

### Task Manager API

```rust
pub struct TaskManager {
    pub fn new() -> Self { ... }
    pub fn create_task(&mut self, task_type: TaskType) -> String { ... }
    pub fn get_task(&self, id: &str) -> Option<&TaskEntry> { ... }
    pub fn list_tasks(&self) -> Vec<&TaskEntry> { ... }
    pub fn update_status(&mut self, id: &str, status: TaskStatus) { ... }
    pub fn output_reader(&self, id: &str) -> OxiResult<impl Iterator<Item=Value>> { ... }
    pub fn kill_task(&mut self, id: &str) -> OxiResult<()> { ... }
}
```

### Notifications

`collect_notifications()` → Vec<TaskNotification>:
- Task started
- Task completed
- Task failed
- Long-running (>30s without output)

Displayed in TUI status bar or CLI output.

**Remote agents** (feature-gated): Task spawned on remote server, streamed back
**Teammate mode** (feature-gated): Task assigned to team member, shared context
**Dream mode** (feature-gated): Task runs continuously, wakes on input

**Source:** `crates/oxicode-tasks/src/manager.rs`, `runner.rs`, `output.rs`, `notifications.rs`

---

## 10. CLI Entry Point (`oxicode-cli`)

Binary dispatcher with multiple operating modes.

### Operating Modes

```rust
enum OperatingMode {
    TUI,                    // Interactive ratatui terminal UI (default)
    NonInteractive,         // Single prompt (-p), exit after response
    Server,                 // JSON-RPC server (--server)
    Daemon,                 // TCP listener for IDE connections (--daemon)
    Bridge,                 // Headless cloud deployment (--bridge)
    Agent,                  // Subagent spawned by parent (--agent-mode)
    Mcp,                    // Expose tools via MCP (--mcp)
}
```

### Flags & Options

```
--model <MODEL>           Override active model
--config-dir <DIR>        Custom config directory
--session <UUID>          Resume previous session
-p, --prompt <MSG>        Single message (non-interactive)
--output json             NDJSON structured output
--completions SHELL       Generate shell completions
--man-page                Generate man page
--skip-onboarding         Skip first-run wizard
```

### Init Sequence

1. **Parse args** → CliArgs
2. **Fast-exit checks** (no state needed):
   - `--agent-mode` → spawn subagent, exit
   - `--completions` → print shell completions, exit
   - `--man-page` → print man page, exit
   - `--mcp` → run MCP server, exit
3. **Setup tracing** → structured logging to `~/.oxicode/logs/`
4. **Load config** → Settings (merge 3 levels)
5. **Load/validate auth** → read ANTHROPIC_API_KEY env or keyring
6. **Build provider router** → LlmProvider (Anthropic, OpenAI-compat, Bedrock, Vertex)
7. **Initialize state** → AppState + StateStore
8. **Initialize engine** → QueryEngine, PermissionPipeline, SessionManager
9. **Load TUI** (if not non-interactive) → ratatui App
10. **Main loop** → handle user input, stream responses, dispatch tools
11. **Save session** → JSON to disk

### Core Types

```rust
pub struct Cli {
    pub model: Option<String>,
    pub config_dir: Option<String>,
    pub session: Option<String>,
    pub prompt: Option<String>,           // -p (non-interactive)
    pub output: OutputFormat,             // text | json
    pub completions: Option<clap_complete::Shell>,
    pub man_page: bool,
    pub agent_mode: Option<String>,
    pub server: bool,
    pub daemon: bool,
    pub bridge: bool,
    pub port: u16,                        // for --bridge
    pub skip_onboarding: bool,
}

enum OutputFormat {
    Text,      // Interactive TUI
    Json,      // NDJSON structured output
}
```

### Slash Commands (100+)

- **Navigation:** `/help`, `/model`, `/session`, `/status`
- **Development:** `/commit`, `/plan`, `/test`, `/debug`, `/review`
- **Team:** `/team`, `/spawn`, `/recruit`
- **Skills:** `/skill activate`, `/skill list`
- **Features:** `/feature toggle vim_mode`
- **Editing:** `/vim` (modal editing), `/compact` (context reduction)
- **Telemetry:** `/telemetry`, `/cost` (billing info)
- **Export:** `/save`, `/load`

See `commands/` directory for full list.

### Structured Output (--output json)

NDJSON format (one JSON per line):

```json
{"type": "streaming_start", "model": "claude-sonnet-4"}
{"type": "text_delta", "text": "Here"}
{"type": "text_delta", "text": " is"}
{"type": "tool_use_start", "tool": "bash", "id": "call_1"}
{"type": "tool_result", "result": "exit 0"}
{"type": "message_complete", "stop_reason": "end_turn"}
```

Used for CI/CD automation, IDE integration, remote agent communication.

### Auth & Telemetry

- **Auth:** ANTHROPIC_API_KEY env → keyring fallback → browser OAuth
- **Telemetry (optional):** OpenTelemetry pipeline; logs to observability backend
- **Voice (feature-gated):** Microphone → Whisper transcription → LLM

**Source:** `crates/oxicode-cli/src/main.rs`, `commands/`, `structured_output.rs`, `auth.rs`, `voice/`

---

## Cross-Subsystem Flow Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│  CLI Entry Point (oxicode-cli/main.rs)                          │
│  ↓ Parse args → Load config → Init state → Run main loop        │
└────────────────────┬──────────────────────────────────────────────┘
                     │
        ┌────────────┴────────────┐
        ↓                         ↓
   TUI (ratatui)          Non-Interactive (-p)
   ↓                        ↓
┌──────────────────────────────────────────┐
│  User Input / Prompt                     │
└────────────┬─────────────────────────────┘
             │
             ↓
┌──────────────────────────────────────────┐
│  Hooks: PreQuery                         │
│  Session Memory: Selector injects chunks │
│  Skills: Activation & inject             │
└────────────┬─────────────────────────────┘
             │
             ↓
┌──────────────────────────────────────────┐
│  QueryEngine (oxicode-core)              │
│  ↓ Forward to LLM provider (oxicode-api) │
└────────────┬─────────────────────────────┘
             │
             ↓
┌──────────────────────────────────────────┐
│  LlmProvider.stream_message()            │
│  (Anthropic, OpenAI, Bedrock, Vertex)    │
│  ↓ Streaming response events             │
└────────────┬─────────────────────────────┘
             │
             ↓
┌──────────────────────────────────────────┐
│  Response Processing                     │
│  ↓ Tool calls detected                   │
└────────────┬─────────────────────────────┘
             │
    ┌────────┴────────────────────┐
    ↓                             ↓
 ┌────────────────┐        ┌──────────────┐
 │ Hooks:         │        │ Built-in     │
 │ ToolCallBefore │        │ Tools (49)   │
 └────────┬───────┘        │ Registry     │
          │                └──────┬───────┘
          │                       │
    ┌─────┴───────────────────────┴─────┐
    │  PermissionPipeline (6 layers)    │
    │  ↓ Safety check → Pattern check   │
    └────────────┬──────────────────────┘
                 │
    ┌────────────┴──────────────┐
    ↓                           ↓
 Allowed                    Needs Approval
 ↓                          (User Dialog)
┌─────────────────────────────────────────┐
│ Tool Execution                          │
│ • Bash → crate oxicode-tools/bash.rs    │
│ • File ops → file_read, file_write      │
│ • Agents → oxicode-agents/spawner       │
│ • MCP tools → oxicode-mcp/manager       │
│ • Plugin tools → oxicode-plugins        │
└────────────┬────────────────────────────┘
             │
             ↓
┌──────────────────────────────────────────┐
│ Hooks: ToolCallAfter                     │
│ State: Update AppState (cost, usage)     │
│ Session: Save conversation              │
└────────────┬─────────────────────────────┘
             │
             ↓
  Tool result → back to LLM for next turn
  (repeat until stop_reason=end_turn)
             │
             ↓
┌──────────────────────────────────────────┐
│ Hooks: PostSampling                      │
│ State: set_streaming(false)              │
│ Session: persist full conversation      │
│ Display: render response in TUI/CLI      │
└──────────────────────────────────────────┘
```

→ See [00-index.md](./00-index.md) for navigation.
