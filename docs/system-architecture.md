# OxiCode — System Architecture

**Version:** 0.5.1 | **Last Updated:** 2026-04-05 | **Phase:** Gap Closure (Phases 7-10 Complete) | **Cumulative:** Phase 1-10

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    TUI Frontend (ratatui)                    │
│  ┌──────────────┬──────────────┬──────────────────────────┐  │
│  │ Status Bar   │              │                          │  │
│  ├──────────────┴──────────────┤   Message View (Stream)  │  │
│  │ Command/Skill Registry      │                          │  │
│  │ Input Box (readline-style)  │                          │  │
│  └─────────────────────────────┴──────────────────────────┘  │
│                                                               │
│  Event Loop: KeyEvents → UiEvent → CoreEvent → StateStore   │
└─────────────────────────────────────────────────────────────┘
                              ▲
                              │ UiEvent (UserInput, Quit)
                              │
┌─────────────────────────────┴─────────────────────────────┐
│                     Core Engine Layer                      │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐ │
│  │ QueryEngine (Main Loop)                              │ │
│  │ - stream_message() → LLM provider                    │ │
│  │ - extract tool uses → execute_tool()                │ │
│  │ - append tool results → recurse until EndTurn       │ │
│  │ - Hook: Context Defense before tool execution       │ │
│  └──────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐ │
│  │ System Prompt Assembly                               │ │
│  │ - Base: hardcoded role + capabilities                │ │
│  │ + Global instructions: ~/.claude/CLAUDE.md           │ │
│  │ + Project instructions: ./.claude/CLAUDE.md          │ │
│  │ + Injected: Skills, Tools, Context Defense Info      │ │
│  └──────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐ │
│  │ Tool Registry & Execution                            │ │
│  │ - 42 built-in tools (Phase 3: 11 new tools added)   │ │
│  │ - Phase 1: MCP resource tools, skill invocation      │ │
│  │ - Phase 3: Team, LSP, PowerShell, REPL, workflow    │ │
│  │ - Permission checks via 6-layer pipeline             │ │
│  │ - MCP bridging for external tools                    │ │
│  └──────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐ │
│  │ Permission Pipeline (6 Layers)                       │ │
│  │ 1. Safe allowlist (read-only) → auto-allow          │ │
│  │ 2. Command security (hard deny — always checks)      │ │
│  │ 3. Pattern detection (always checked, bypass warns)  │ │
│  │ 4. Permission mode check (Bypass/ApprovalOnly)       │ │
│  │ 5. Rule matching (user-configured)                   │ │
│  │ 6. Default → Ask user                                │ │
│  └──────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                              ▲
                              │ Tool requests
                              │
┌─────────────────────────────┴─────────────────────────────┐
│             Phase 4: Context Defense & Multi-Agent         │
│                                                             │
│  ┌────────────────────────────────────────────────────┐  │
│  │ Context Defense (5 Layers)                         │  │
│  │ - L1: Truncate oldest middle messages              │  │
│  │ - L2: Compress tool results + thinking blocks      │  │
│  │ - L3: Auto-summarize (LLM-assisted @ 70% budget)   │  │
│  │ - L4: Reactive compact (emergency @ 95% budget)    │  │
│  │ - L5: Context collapse (reset @ 100% budget)       │  │
│  │ Orchestrated by: BudgetManager                     │  │
│  └────────────────────────────────────────────────────┘  │
│                                                             │
│  ┌────────────────────────────────────────────────────┐  │
│  │ Multi-Agent System                                  │  │
│  │ - Spawner: Launch subagents w/ config               │  │
│  │ - Coordinator: Manage agent team + delegation       │  │
│  │ - MessageBus: Inter-agent JSON messaging            │  │
│  │ - Team: Shared state, collective operations         │  │
│  └────────────────────────────────────────────────────┘  │
│                                                             │
│  ┌────────────────────────────────────────────────────┐  │
│  │ Skill System                                        │  │
│  │ - Discovery: Load SKILL.md from ~/.oxicode/skills   │  │
│  │ - Parser: Extract YAML metadata + prompt text      │  │
│  │ - Executor: Inject skill prompts on activation     │  │
│  │ - Activation: File type, user intent, keyword      │  │
│  └────────────────────────────────────────────────────┘  │
│                                                             │
│  ┌────────────────────────────────────────────────────┐  │
│  │ Background Task Management                         │  │
│  │ - TaskManager: In-process registry                  │  │
│  │ - TaskRunner: Async process spawning                │  │
│  │ - OutputReader: Incremental JSONL streaming        │  │
│  │ - NotificationCollector: Task status updates        │  │
│  └────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              ▲
                              │ API calls
                              │
┌─────────────────────────────┴─────────────────────────────┐
│            Multi-Provider LLM Interface Layer              │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐ │
│  │ Provider Router                                      │ │
│  │ - Load balance across providers                     │ │
│  │ - Auto-detect from env vars                         │ │
│  │ - Fallback on errors                                │ │
│  └──────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐ │
│  │ Supported Providers                                 │ │
│  │ - Anthropic (Claude)       - OpenAI (GPT-4o)        │ │
│  │ - OpenAI Compatible APIs   - AWS Bedrock            │ │
│  │ - Google Vertex AI         - MCP Server             │ │
│  │                                                     │ │
│  │ See phase-2-api-enhancement.md for provider details │ │
│  └──────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                              ▲
                              │ HTTP/SSE
                              │
┌─────────────────────────────┴─────────────────────────────┐
│                  External LLM Services                     │
│                                                             │
│  [api.anthropic.com]  [api.openai.com]  [localhost:3000]  │
└─────────────────────────────────────────────────────────────┘
```

---

## Data Flow: Query Execution

### Step 1: User Input
```
User types in TUI input box → UiEvent::UserInput("what is foo?")
```

### Step 2: Message Assembly
```
StateStore.messages += new Message {
  role: User,
  content: [Text { text: "what is foo?" }],
  created_at: Utc::now(),
}
```

### Step 3: System Prompt Assembly
```
QueryEngine::assemble_system_prompt():
  base + global_instructions + project_instructions
  + skill_descriptions + tool_schemas
```

### Step 4: LLM Request
```
provider.stream_message(MessageRequest {
  model: "claude-3-5-sonnet-20241022",
  messages: [...conversation...],
  system: assembled_prompt,
  max_tokens: 4096,
  tools: [...schemas...],
})
```

### Step 5: Stream Processing
```
loop {
  event = stream.next().await?
  match event {
    TextDelta { text } → append_to_message
    ToolUseStart { name } → create_tool_use_block
    ToolInputDelta { json } → append_to_tool_input
    UsageUpdate(usage) → update_token_tracking
    MessageStop { reason } → break_on_EndTurn
  }
}
```

### Step 6: Context Defense Check (Phase 4)
```
if message.stop_reason == ToolUse {
  BudgetManager.check_budget():
    tokens_used = count_tokens(conversation)
    ratio = tokens_used / model_max_tokens
    
    if ratio >= 1.0 {
      L5: ContextCollapse → reset_from_disk
    } else if ratio >= 0.95 {
      L4: ReactiveCompact → summarize_during_stream
    } else if ratio >= 0.90 {
      L3: AutoCompact → trigger_async_summarization
    }
}
```

### Step 7: Tool Extraction & Permission Check
```
if stop_reason == ToolUse {
  for tool_use in extract_tool_uses(message) {
    decision = permission_pipeline.check(
      tool_name,
      tool_level,
      input_json,
    )
    
    match decision {
      Allow → execute_tool(...)
      Deny(reason) → append_error_block
      Ask(prompt) → (TODO: TUI prompt user)
    }
  }
}
```

### Step 8: Tool Execution
```
tool_result = tool_registry.execute(name, input, context)
conversation += Message {
  role: User,
  content: [ToolResult {
    tool_use_id: id,
    content: result,
    is_error: false,
  }],
}
```

### Step 9: Loop Back
```
If token_ratio < 1.0 && tool_result provided:
  goto Step 4 (another LLM turn)
else:
  render_message_complete() → StateStore update
```

---

```
IDE Extension ←──stdin/stdout──→ oxicode --server
                                    │
                                    ├── Session management
                                    ├── Tool execution
                                    ├── Streaming responses
                                    └── JSON-RPC 2.0 protocol
```

**Phase 6 updates:**
- Added top-level Server Mode entry point (Phase 6: Server Mode + IDE Bridge)
- Server loop handles JSON-RPC over stdin/stdout
- Per-session tokio tasks with cancellation support
- Permission bridge: ask → wait → respond flow
- Streaming notifications: stream.text, tool.start, tool.result, permission.ask

---

## Phase 1: New Tools & Features

### MCP Resource Tools
**Purpose:** Access resources exposed by connected MCP servers

- `list_mcp_resources` — List all available resources from MCP servers (with optional server filter)
- `read_mcp_resource` — Read content of a specific MCP resource by URI

**Integration:** Implemented in `oxicode-tools/src/mcp_resource_tools.rs`, permission level: ReadOnly

### Skill Tool
**Purpose:** Invoke discovered skills by name within the conversation

- `skill` — Execute a skill and inject its prompt content
  - Input: skill name (e.g., "commit", "review-pr"), optional args
  - Output: Skill prompt text or error if not found
  - Trust model: Skills are user-installed from trusted discovery paths only

**Integration:** Implemented in `oxicode-tools/src/skill_tool.rs`, permission level: ReadOnly

### Enhanced /compact Command
**Previous:** Stub command, showed "requires interactive mode"

**Now:** Async LLM-assisted summarization
- User runs `/compact` in TUI
- Triggers async LLM task: "Summarize conversation into 1-2 sentences"
- Returns control immediately to TUI (non-blocking)
- Updates `StateStore` via `replace_messages()` when summarization completes

**Implementation:** CompactCommand calls async task in main.rs, uses new `StateStore::replace_messages()` method

---

## Phase 2: Memory Extraction Service

### Memory Selection (Relevance-Based)
**Purpose:** Inject top 5 relevant memories from historical project knowledge into system prompt per query.

**Flow:**
1. `QueryEngine` calls `MemorySelector::select_relevant(query, all_memories)`
2. Selector builds manifest of 200-cap memories with indices + summaries
3. Calls LLM (Haiku) with selection prompt: "Pick indices of top {N} relevant to this query"
4. Parses JSON response, returns selected `MemoryEntry` objects
5. Falls back to recency-based selection if LLM fails

**Integration:** `oxicode-core/src/system_prompt.rs` assembles `relevant_memories` parameter into context

**Implementation:** `oxicode-session/src/memory_selector.rs` (~200 LOC, `SelectionLlm` trait for testability)

### Memory Extraction (Auto-Extract)
**Purpose:** Persistently capture decisions, preferences, context from conversation at session end.

**Flow:**
1. Session ends (user quit or `/save`)
2. `SessionEnd` hook triggers `MemoryExtractor::extract_and_save(messages, session_id)`
3. Pattern-based extraction: identifies decisions ("decided to", "will use"), preferences ("prefer X over Y"), context facts
4. Deduplicates against existing memories by content similarity
5. Caps at 10 memories per session to avoid noise
6. Saves to project memdir as `.md` files with YAML frontmatter + content

**Implementation:** `oxicode-session/src/memory_extractor.rs` (~250 LOC)

**Data format:**
```yaml
---
id: mem-001
type: Decision
created_at: 2026-04-05T12:34:56Z
session_id: sess-123
tags: [architecture, testing]
---

Decided to use property-based testing for integer ranges.
```

### Memory Freshness (Age Caveats)
**Purpose:** Warn about stale memories so LLM questions older information.

**Rules:**
- < 1 day old: no caveat
- 1-7 days old: append `"(X days ago)"`
- > 7 days old: append `"(X days ago — verify before using)"`

**Implementation:** `oxicode-session/src/memory_freshness.rs` (~80 LOC, called before injection)

### Team Memory Sync (Feature-Gated)
**Purpose:** Share project memories across team members via delta-sync (bandwidth-efficient).

**Flow (if feature `team_memory_sync` enabled):**
1. On config, compute SHA-256 for each local memory
2. POST to team endpoint: `{ project_id, checksums: [...] }`
3. Endpoint responds with missing/outdated memory IDs
4. Download missing memories, upload new ones
5. Cache response with TTL

**Configuration:** `settings.toml`
```toml
[team]
memory_sync_url = "https://team.example.com/api/team_memory/sync"
```

**Implementation:** `oxicode-session/src/team_memory_sync.rs` (~300 LOC, feature-gated)

---

## Phase 4: Context Defense Layers

### Layer 1: Truncation (Eager, L1)
**Trigger:** token_ratio >= 0.70 (70% budget used)

**Action:** Remove oldest middle messages until ratio < 0.65

**Trade-off:** Loses historical context, keeps recent conversation

```rust
pub fn truncate_messages(
    messages: &mut Vec<Message>,
    current_tokens: usize,
    max_tokens: usize,
) -> usize {
    // Remove middle messages (preserve first + last)
}
```

### Layer 2: Microcompaction (Eager, L2)
**Trigger:** L1 insufficient, or concurrent with L1

**Action:** In-place compression:
- Collapse consecutive identical blocks
- Remove XML whitespace from thinking blocks
- Abbreviate long tool results (keep first 100 chars + "...")

**Trade-off:** Lossy, but preserves structure for tool/thinking blocks

```rust
pub fn microcompact_messages(messages: &mut Vec<Message>);
```

### Layer 3: Auto-Compaction (Async, L3)
**Trigger:** token_ratio >= 0.70 AND triggered_before in same session

**Action:** Spawn async task to ask LLM: "Summarize conversation into 1-2 sentences"

**Trade-off:** Blocks on LLM API call, but async so doesn't block user

```rust
pub struct AutoCompactor {
    provider: Arc<dyn LlmProvider>,
}

impl AutoCompactor {
    pub async fn summarize(
        &self,
        messages: &[Message],
        max_tokens: u32,
    ) -> OxiResult<String>;
}
```

### Layer 4: Reactive Compaction (Mid-Stream, L4)
**Trigger:** token_ratio >= 0.95 (95% budget) DURING streaming

**Action:** If provider reports tokens approaching limit:
- Interrupt stream collection
- Apply L1 + L2 aggressively
- Resume

**Trade-off:** May lose output in flight, triggers only in extreme cases

```rust
pub struct ReactiveCompactor {
    should_trigger: Rc<AtomicBool>,
}

impl ReactiveCompactor {
    pub fn should_trigger(&self, tokens: usize, max: u32) -> bool;
}
```

### Layer 5: Context Collapse (Hard Stop, L5)
**Trigger:** token_ratio >= 1.0 (100% budget) or conversation unrecoverable

**Action:** Save entire conversation to disk, return empty history

**Trade-off:** Loses entire conversation, starts fresh

```rust
pub struct ContextCollapse {
    save_dir: PathBuf,
}

impl ContextCollapse {
    pub async fn reset_and_save_state(&self, state: &AppState) -> OxiResult<()>;
}
```

---

## Phase 4: Multi-Agent System

### Agent Spawning Flow

```
User command: /agent spawn "research" --model=sonnet --tools=search,web_fetch
  ↓
AgentConfig {
  name: "research",
  model: "claude-3-5-sonnet-20241022",
  tools: ["search", "web_fetch"],
  max_tokens: 4096,
  system_prompt: <optional override>,
}
  ↓
Spawner::spawn_agent(config)
  ↓
Create child process: oxicode --agent-mode
Write config JSON to stdin
  ↓
AgentHandle {
  id: UUID,
  name: "research",
  status: Running,
  child: Child,
  rx: watch::Receiver<AgentStatus>,
}
  ↓
Coordinator can now send/receive messages via MessageBus
```

### Inter-Agent Communication

```
CoordinatorState {
  agents: HashMap<AgentId, AgentInfo>,
  message_bus: Arc<MessageBus>,
}

MessageBus::send(
  from: AgentId,
  to: AgentId,
  message: AgentMessage {
    id: UUID,
    body: "please research X",
    task_id: Optional<TaskId>,
  },
)

// Agent polls message_bus for incoming messages
// Handles asynchronously, responds with AgentMessage::Reply
```

### Agent Lifecycle
1. **Init:** Load config, initialize state
2. **Run:** Execute QueryEngine loop (same as main OxiCode)
3. **Tool Execution:** Can call coordinator tools (send_message, request_synthesis)
4. **Shutdown:** Graceful exit on signal, cleanup resources

---

## Phase 4: Skill System

### Skill File Format

```yaml
# ~/.oxicode/skills/my-skill/SKILL.md

---
name: "Python Debugger"
version: "1.0"
triggers:
  file_type: ["*.py"]
  keywords: ["debug", "error", "traceback"]
  user_intent: ["fix", "understand"]
depends_on: []
---

# Debugging Python Code

When you see a Python error, follow this checklist:
1. Read the traceback line-by-line
2. Identify the failing line in context
3. Suggest a fix
4. Provide a corrected code snippet
```

### Discovery Process

```
SkillDiscovery::new(
  user_dir: ~/.oxicode/skills,
  project_dir: ./.oxicode/skills,
)
  ↓
Walk both directories, find all SKILL.md files
  ↓
Parse each:
  - Extract YAML frontmatter
  - Validate schema
  - Store (name, version, triggers, prompt_text)
  ↓
SkillExecutor::build_skills_prompt(context)
  ↓
For each skill, check if activation triggers match:
  - current_file matches file_type glob?
  - user_input contains keywords?
  - user_intent in triggers?
  ↓
Inject matching skill prompts into system_prompt
```

---

## Phase 4: Background Task Management

### Task Lifecycle

```
User: /task run "cargo test" --background
  ↓
TaskManager::create(TaskEntry {
  id: UUID,
  task_type: ShellCommand,
  command: "cargo test",
  created_at: now,
  status: Pending,
})
  ↓
TaskRunner::spawn(task_id, command)
  ↓
Child process spawned, stdout/stderr redirected to disk:
  {work_dir}/.oxicode/tasks/{task_id}/output.jsonl
  ↓
Each line written as: {"time":"...", "stream":"stdout", "data":"..."}
  ↓
OutputReader polls file, yields lines as they arrive
  ↓
NotificationCollector dedupes:
  - "Still running" max 1x per minute
  - "Completed" once
  - Errors logged
  ↓
UI shows: [▀ cargo test (120s elapsed)] in task_panel
  ↓
User clicks: /task view {id}
  ↓
Read entire output.jsonl, render scrollable log
```

### Output Streaming

```
TaskRunner writes JSONL:
```
{"time":"2026-04-02T08:00:00Z","stream":"stdout","data":"running 5 tests\n"}
{"time":"2026-04-02T08:00:01Z","stream":"stderr","data":"thread panicked\n"}
{"time":"2026-04-02T08:00:02Z","stream":"stdout","data":"test passed\n"}
{"time":"2026-04-02T08:00:05Z","stream":"exit_code","data":"0"}
```

OutputReader:
```rust
OutputReader::next() -> OxiResult<OutputLine> {
    // Read next JSONL line
    // Deserialize
    // Return OutputLine { timestamp, stream, data }
}
```

---

## Phase 6: Server Mode + IDE Bridge

### Protocol: JSON-RPC 2.0 over stdin/stdout

**Message Format (bidirectional line-delimited JSON):**

```json
// IDE → OxiCode (Request with ID for correlation)
{"jsonrpc":"2.0","id":"uuid","method":"session.create","params":{"user_id":"alice"}}

// OxiCode → IDE (Response with matching ID)
{"jsonrpc":"2.0","id":"uuid","result":{"session_id":"sess-123"}}

// OxiCode → IDE (Notification, no ID)
{"jsonrpc":"2.0","method":"stream.text","params":{"delta":"Hello"}}
```

### Supported Methods

**Requests (IDE → OxiCode):**
| Method | Params | Response | Purpose |
|--------|--------|----------|---------|
| session.create | user_id | session_id | Start new session |
| session.resume | session_id | session_id | Resume existing |
| session.list | — | [sessions] | List available |
| message.send | session_id, text | — | Send user message (streams notifications) |
| message.cancel | request_id | — | Cancel in-flight request |
| tool.approve | request_id, decision | — | Approve pending permission |
| tool.deny | request_id, reason | — | Deny pending permission |
| compact | session_id | — | Trigger LLM-assisted compaction |
| shutdown | graceful | — | Graceful server shutdown |

**Notifications (OxiCode → IDE):**
| Method | Params | Meaning |
|--------|--------|---------|
| stream.text | delta | LLM text delta |
| tool.start | name, input | Tool execution started |
| tool.result | name, output | Tool execution completed |
| permission.ask | tool_name, justification | User permission needed |
| session.updated | session_id, state | Session state changed |
| error | code, message | Error notification |

### Session Management

```
ServerHandler {
  sessions: HashMap<SessionId, SessionState>,
  message_tx: mpsc::Sender<ServerNotification>,
}

SessionState {
  id: SessionId,
  user_id: UserId,
  query_engine: Arc<QueryEngine>,
  cancellation_token: CancellationToken,
  permission_pending: Option<PermissionRequest>,
  timeout: Duration,
}
```

**Lifecycle:**
1. IDE: `session.create` → ServerHandler creates SessionState + tokio task
2. IDE: `message.send` → SessionState spawns query_engine.stream_message() with streaming callback
3. Streaming callback emits `stream.text`, `tool.start`, `tool.result` notifications
4. On permission request: emit `permission.ask`, block until `tool.approve`/`tool.deny`
5. IDE: `session.resume` → Lookup SessionState, continue using same query_engine

### Permission Bridge Flow

```
query_engine requests permission
  ↓
Streaming callback triggers permission_pending = Some(request)
  ↓
Emit notification: {"method":"permission.ask","params":{...}}
  ↓
IDE displays prompt to user
  ↓
IDE sends: {"method":"tool.approve","params":{"request_id":"x"}}
  ↓
ServerHandler unblocks query_engine
  ↓
Tool execution resumes, result emitted as notification
```

### Cancellation

```
IDE: message.cancel {request_id}
  ↓
ServerHandler finds SessionState
  ↓
Calls cancellation_token.cancel()
  ↓
tokio task observing token gets cancelled
  ↓
In-flight query_engine request interrupted
  ↓
Emit notification: stream ends, tool results discarded
```

### Implementation Files

- **`server_protocol.rs`** — JSON-RPC types, serialization
- **`server.rs`** — Main server loop, stdin/stdout handler
- **`server_handler.rs`** — RequestHandler, session management, streaming
- **`main.rs`** — `--server` flag routing

### Bridge Expansion (9 modules)

The bridge layer (`crates/oxicode-cli/src/bridge/`) provides IDE integration via JSON-RPC:

| Module | Purpose |
|--------|---------|
| `mod.rs` | Transport enum (Stdio/TCP/WebSocket), protocol version, capabilities |
| `messages.rs` | JSON-RPC method + notification definitions |
| `session_bridge.rs` | Session create/resume/list/save lifecycle |
| `session_ingress.rs` | HMAC-SHA256 token auth for session routing |
| `permission_bridge.rs` | IDE permission request/response flow (60s timeout) |
| `daemon_listener.rs` | TCP listener + lockfile for daemon mode |
| `bridge_config.rs` | Config from `[bridge]` in settings.toml + env var overrides |
| `bridge_debug.rs` | Message logging to `~/.oxicode/bridge-debug.log` (10MB rotation) |
| `bridge_status.rs` | Connection state (Connected/Disconnected/Reconnecting), uptime, message counts |
| `reconnection.rs` | Exponential backoff (1s→30s cap, ±10% jitter, max 100 attempts) |

### Hook Execution Modes (3 types)

The hooks system (`crates/oxicode-hooks/`) supports 3 executor types dispatched via `HookType`:

```
Event fires → HookManager.fire(event, data)
  ↓
  Config.get(event) → HookDef { hook_type, ... }
  ↓
  match hook_type:
    Command → spawn `sh -c {command}`, JSON stdin/stdout (10s timeout)
    Agent   → LLM call with structured output (60s timeout, stub)
    Http    → POST to URL with SSRF guard (10min timeout)
  ↓
  Parse response: Pass | ModifyPrompt | Abort | OverrideResult
  ↓
  Any error → Pass (fail-open, never blocks user)
```

**SSRF protection:** Rejects private IPs (10.x, 172.16-31.x, 192.168.x, 127.x, ::1, fe80::/10, fc00::/7, ::ffff:private).

---

## State Management

### AppState (Central Source of Truth)

```rust
pub struct AppState {
    pub session_id: String,
    pub messages: Vec<Message>,          // Conversation history
    pub is_streaming: bool,               // Currently receiving from LLM
    pub current_model: String,            // Active model
    pub total_usage: Usage,               // Cumulative tokens
    
    // Phase 4 additions (extended in AppState):
    pub agents: HashMap<AgentId, AgentInfo>,
    pub skills: Vec<SkillInfo>,
    pub background_tasks: HashMap<TaskId, TaskStatus>,
    pub context_budget: BudgetStatus,
}
```

### StateStore (Watch Channel)

```rust
pub struct StateStore {
    tx: watch::Sender<AppState>,
    rx: watch::Receiver<AppState>,
}

impl StateStore {
    pub fn subscribe(&self) -> watch::Receiver<AppState>;
    pub fn current(&self) -> AppState;
    pub fn update<F>(&self, f: F) where F: FnOnce(&mut AppState);
}
```

**Pattern:** All state mutations go through `StateStore::update()`, subscribers notified via watch channel.

---

## Error Handling

### Error Type Hierarchy

```rust
pub enum OxiError {
    // API errors (retryable)
    Api { message, status, retryable },
    
    // Config errors (non-retryable)
    Config(String),
    
    // Tool execution errors
    Tool { name, message },
    
    // Permission denied
    Permission(String),
    
    // Session/state errors
    Session(String),
    
    // Standard library errors
    Io(io::Error),
    Json(serde_json::Error),
    
    // TUI errors
    Tui(String),
    
    // Stream ended unexpectedly
    StreamClosed,
    
    // Catch-all
    Other(String),
}
```

**Handling Pattern:**
```rust
match result {
    Ok(msg) → update UI
    Err(e) if e.is_retryable() → retry with backoff
    Err(e) → log error, show user message, continue
}
```

---

## Integration Points Summary

| Layer | Component | Integration |
|-------|-----------|-------------|
| **TUI** | App event loop | Send UiEvent → CoreEvent → StateStore |
| **Core** | QueryEngine | Stream from provider, execute tools, catch errors |
| **Tools** | ToolRegistry | Check permissions before execute, capture output |
| **Permissions** | Pipeline | 6-layer check before tool execution |
| **Context** | BudgetManager | Hook into stream loop, apply L1-L5 defenses |
| **Multi-Agent** | Spawner | Launch child process with config, manage handle |
| **Skills** | SkillExecutor | Inject into system prompt on activation |
| **Tasks** | TaskRunner | Spawn process, redirect I/O, poll OutputReader |

---

## Performance Characteristics

| Operation | Latency | Notes |
|-----------|---------|-------|
| LLM response | 2-10s | Depends on model, prompt size |
| Tool execution | <500ms | File I/O, bash, grep typically fast |
| Permission check | <1ms | Local 6-layer pipeline |
| L1 truncation | <10ms | O(n) where n=message count (~100) |
| L2 microcompact | <50ms | O(n*m) where m=avg content length |
| L3 auto-summarize | 5-10s | Async LLM call |
| L4 reactive | <100ms | Must complete during stream |
| L5 collapse | <50ms | Save to disk only |
| UI frame render | <100ms | ratatui terminal write |
| Agent spawn | <200ms | Process creation overhead |

---

## Security Model

### Input Validation
1. **Tool inputs:** JSON schema validation before execution
2. **File paths:** Path traversal check, no `../` outside working dir
3. **Shell commands:** Dangerous pattern detection (rm -rf, >;, etc)
4. **Task IDs:** UUID validation, alphanumeric+hyphens only

### Output Sanitization
1. **Secrets redaction:** Pattern match API keys, passwords
2. **XML escaping:** Tool results in XML → escape `<>"`
3. **Ansi stripping:** Some outputs redacted before display

### Access Control
1. **Tool level:** ReadOnly → FileWrite → ShellExec → System
2. **Permission mode:** Default (ask) → ApprovalOnly (always ask) → Bypass (dev)
3. **Rules:** User-configured allow/deny for specific tools/patterns

---

---

## Phase 8: UX Polish — Vim Mode, Keybindings, Onboarding

### Vim Mode State Machine

Fully stateful vim emulation with 4 modes: Normal, Insert, Visual, Command.

**State Machine:**
```
Insert ←──→ Normal ←──→ Visual
            ↓
          Command
```

**Implementation:** `oxicode-tui/src/vim_mode.rs`
- Tracks current mode, last search pattern, count prefix
- Parses motion commands: hjkl, w/b/e, 0/$, gg/G
- Operators: dd, yy, dw, cw, x, D, C, p, u
- Search (/) with incremental find, command mode (:q, :w, :wq)
- Count prefix support (3l, 5j)
- Mode indicator in StatusBar + InputBox visual feedback

**Integration Points:**
- `InputBox::handle_key_event()` — Route events to vim state machine
- `StatusBar` — Display current mode (-- INSERT --, -- VISUAL --)
- Settings toggle: `/vim` command or config.editor_mode = "vim"

---

## Phase 6: TUI Advanced Dialogs & Vim Text Objects (NEW)

### Vim Text Objects

**Implementation:** `oxicode-tui/src/vim_text_objects.rs` (NEW in Phase 6)

**Text object syntax:** `{action}{text_object}` where:
- **Actions:** d (delete), c (change), y (yank)
- **Objects:** 
  - `iw` — inner word (word content)
  - `aw` — a word (word + surrounding whitespace)
  - `i"` — inner double quotes
  - `a"` — a double quotes (including quotes)
  - `i(` — inner parentheses
  - `a(` — a parentheses (including parens)
  - `i{` — inner braces
  - `a{` — a braces (including braces)

**Examples:**
```
diw  → Delete inner word
ci"  → Change inner quoted string
ya{  → Yank around braces
```

**Integration:** Works with operator composition, count prefixes (3diw)

---

## Phase 5: Plugin Marketplace & Enterprise Settings

### Plugin Registry Client (`oxicode-plugins`)

**Purpose:** Remote index fetch/cache, search, version filtering, download for marketplace plugins

**Architecture:**
```
PluginRegistry (client)
  ├── fetch_index() → Remote registry (e.g., GitHub)
  ├── cache_index() → ~/.oxicode/plugin-cache/{hash}
  ├── search(query) → Filter by keywords, trust level
  ├── filter_by_version(plugin, constraint) → Semver range
  └── download_archive(url) → tar.gz → extract
```

**Key Types:**
- `PluginEntry` — name, version, download_url, trust (verified/community/unverified), permissions, min_oxicode_version
- `PluginManager` — Orchestrates lifecycle: discovery → validation → installation → loading
- `TrustLevel` — Verified (signed, admin), Community (community voted), Unverified (default)

**Features:**
- **Hot-Reload:** `/reload-plugins` command triggers `manager.reload_plugins()`
- **Install from Registry:** Downloads tar.gz, extracts, validates manifest, loads
- **Permission Manifest:** Each plugin declares tools + hooks it needs
- **Cache TTL:** 1-hour default, respects `OXICODE_PLUGIN_CACHE_DIR` env var

**Files (oxicode-plugins crate, 2.1K LOC):**
- `registry.rs` — Remote index client, caching, search/filter APIs
- `manager.rs` — Plugin lifecycle, validation, hot-reload
- `security.rs` — Trust assessment, permission validation
- `manifest.rs` — Plugin.toml parsing (name, version, permissions)
- `install.rs` — Download, extract, install flow
- `lifecycle.rs` — Enable/disable state machine
- `subprocess.rs` — Subprocess plugin spawning (sandboxing ready)

**Commands (CLI integration):**
```
/plugin browse [--category TAG]        # List marketplace plugins
/plugin search QUERY                   # Search by keywords
/plugin info NAME [--version V]        # Details + trust level
/plugin install NAME[@VERSION]         # Download + install
/plugin update [NAME]                  # Update all or specific
/plugin remove NAME                    # Uninstall
/plugin list [--status]                # Local installed plugins
/reload-plugins                        # Hot-reload running plugins
```

### Enterprise Managed Settings (`oxicode-config`)

**Purpose:** Remote admin endpoint for enterprise settings, HMAC-validated, cloud sync-capable

**Architecture:**
```
EnterpriseSettingsClient
  ├── endpoint: String (OXICODE_ENTERPRISE_SETTINGS_URL)
  ├── signing_key: Option<String> (env: OXICODE_ENTERPRISE_KEY)
  ├── fetch() → HTTP GET + HMAC-SHA256 validation
  ├── cache_dir: ~/.oxicode/enterprise-cache
  └── TTL: 1 hour (configurable)
```

**Data Model:**
```rust
EnterpriseSettingsResponse {
  settings: HashMap<String, String>,     // key-value pairs
  locked: HashMap<String, bool>,         // Locked keys (immutable)
  signature: String,                      // HMAC-SHA256 (hex)
  version_ts: Option<String>,             // Admin version timestamp
}
```

**Validation Flow:**
1. Fetch settings from remote endpoint
2. Extract signature from response
3. Compute HMAC-SHA256(`signing_key`, JSON payload)
4. Compare with response.signature (hex match)
5. Cache locally if valid
6. Return merged (enterprise + user local) on next request

**Cloud Sync Integration:**
- `push_settings()` — OAuth-protected upload of user settings to cloud
- `pull_settings()` — Fetch remote user settings (with OAuth token)
- `sync_status()` — Compare hashes, detect conflicts
- **Conflict Resolution:** Latest-wins with logging of overridden keys

**Files (oxicode-config):**
- `enterprise_settings.rs` — Client, caching, signature validation
- Updated `settings.rs` — Merge enterprise + local settings
- `mdm.rs` — Platform MDM layer (existing, used by enterprise)

**Environment Variables:**
- `OXICODE_ENTERPRISE_SETTINGS_URL` — Admin endpoint (e.g., `https://admin.company.com/settings`)
- `OXICODE_ENTERPRISE_KEY` — HMAC signing key (shared secret)
- `OXICODE_ENTERPRISE_CACHE_TTL` — Cache staleness (seconds, default 3600)

**New Dependencies:**
- `flate2` — gzip compression for plugin archives
- `tar` — Archive extraction
- `hmac`, `sha2`, `hex` — Enterprise signature validation
- `reqwest` (enhanced) — Plugin registry + enterprise endpoint fetches
- `chrono` (enhanced) — Timestamp handling in cache/version tracking

### Config Migrations (`oxicode-config/src/migrations.rs`)

**Purpose:** Auto-run versioned migrations on startup to handle config schema evolution, model name renames, and format upgrades without data loss.

**Architecture:**
```
Migration {
  version: u32,
  name: &str,
  apply: fn(&mut Value) -> Result<()>
}

MigrationRunner
  ├── Load raw config (TOML → serde_json::Value)
  ├── Compare current config_version with latest migration
  ├── Auto-detect pending migrations
  ├── Create backup: {config_path}.bak.{timestamp}
  ├── Apply each pending migration in order (idempotent)
  ├── Write updated config with new config_version
  └── Log results + rollback support
```

**Built-in Migrations:**
1. **v0 → v1:** Bootstrap — Add `config_version: u32` field (default 0)
2. **v1 → v2:** Model aliases — Rename deprecated model names (e.g., `claude-3-sonnet` → `claude-sonnet-4-5`)
3. **v2 → v3:** Default features — Insert missing features section if absent
4. **v3 → v4:** Permission mode — Normalize permission_mode values

**Startup Flow:**
```
App::startup()
  ↓
Load raw config from disk
  ↓
MigrationRunner::run_migrations()
  ├── Create backup if first migration
  ├── Apply v1, v2, v3, v4 (only pending ones)
  ├── Update config_version field
  ├── Log "Applied N config migrations (v{old} → v{new})"
  └── Return migrated config as Value
  ↓
Parse Value → Settings struct (now safe, schema matches code)
  ↓
Proceed with rest of startup
```

**Error Handling:**
- If migration fails: Log error, fallback to original config (don't crash)
- Backup enables manual recovery if needed
- In-memory variant supports tests and programmatic migrations

**Files:**
- `crates/oxicode-config/src/migrations.rs` — Framework + 4 built-in migrations (~280 LOC)
- `crates/oxicode-config/src/settings.rs` — Added `config_version: u32` field
- `crates/oxicode-config/src/lib.rs` — Integrated `run_migrations()` before TOML parsing

**Test Coverage:**
- 54/54 oxicode-config tests passing
- 1094 full workspace tests passing
- Includes: framework mechanics, each migration, integration tests

---

## Phase 7: Voice, Bridge, Telemetry & GitHub Integration

### Voice Input Module (`voice/` — feature-gated: `voice`)

**Purpose:** Real-time voice capture and speech-to-text via Whisper API

**Architecture:**
```
Microphone (cpal)
  ↓
Audio Buffer (PCM)
  ↓
VAD (Voice Activity Detection)
  ↓
Whisper API
  ↓
Transcription (text)
```

**Key components:**
- **`audio_capture.rs`** — cpal microphone device management, PCM buffer handling
- **`vad.rs`** — Voice activity detection (silence detection, noise filtering)
- **`whisper_client.rs`** — Async Whisper API calls, retry logic
- **`voice_command_parser.rs`** — Parse transcribed text as slash commands or user input

**Commands:**
- `/voice on` — Enable microphone capture, start listening
- `/voice off` — Disable capture, stop listening
- `/voice status` — Show current state (listening/idle/error)

**TUI Integration:**
- Status bar shows 🎤 indicator with color state:
  - Green: actively listening
  - Yellow: processing
  - Red: error or disabled

**Feature Flag:** Add to `Cargo.toml`:
```toml
[features]
voice = ["cpal", "hound"]
```

**Dependencies:** cpal (audio), hound (WAV), reqwest (Whisper API)

---

### Remote Bridge Mode (`remote/` — feature-gated: `bridge`)

**Purpose:** Bridge OxiCode sessions over WebSocket, enabling multi-device control

**Architecture:**
```
Local OxiCode Instance
  ↓ (WebSocket)
Bridge Server (remote endpoint)
  ↓ (session pool)
Remote OxiCode Instance
  ↓
Local LLM/Services
```

**Key components:**
- **`bridge_server.rs`** — WebSocket server, session routing
- **`bridge_client.rs`** — Client to connect to remote bridge endpoint
- **`session_pool.rs`** — Manage multiplexed sessions, JWT auth
- **`message_relay.rs`** — Relay QueryEngine messages between endpoints
- **`auth.rs`** — JWT token generation + validation

**Commands:**
- `/remote-setup` — Configure bridge endpoint (URL, optional auth)
- `/bridge [port]` — Start local bridge server on port (default 8765)
- `/remote-env list` — List remote environment variables
- `/remote-env set KEY VALUE` — Set remote env var (persisted)

**Features:**
- **Session Pool:** Multiple concurrent sessions with separate JWT tokens
- **Message Relay:** Tool results, LLM responses, state updates streamed back
- **Error Recovery:** Automatic reconnect on timeout, graceful degradation
- **Env Sync:** Remote environment variables accessible to tools

**Feature Flag:**
```toml
[features]
bridge = ["tokio-tungstenite", "jsonwebtoken"]
```

**Dependencies:** tokio-tungstenite (WebSocket), jsonwebtoken (JWT), uuid

---

### Telemetry Pipeline (`telemetry_pipeline/` — feature-gated: `telemetry-otlp`)

**Purpose:** Structured event collection and export via OpenTelemetry (OTLP protocol)

**Architecture:**
```
LLM Events (token counts, latency)
Tool Events (execution time, errors)
User Events (commands, tool uses)
  ↓
Event Collector
  ↓
├─ Local NDJSON Logger (~/.oxicode/telemetry/*.jsonl)
└─ OTLP Exporter (configurable endpoint)
  ↓
Telemetry Backend (Jaeger, DataDog, etc)
```

**Key components:**
- **`collector.rs`** — Event collection, buffering, batching
- **`event.rs`** — Event schema (OxiTelemetryEvent with timestamp, type, metadata)
- **`ndjson_logger.rs`** — Local persistence (NDJSON format with rotation)
- **`otlp_exporter.rs`** — OpenTelemetry Protocol (OTLP) HTTP exporter
- **`metrics.rs`** — Metric aggregation (counters, histograms, gauges)

**Event Types:**
```rust
pub enum TelemetryEventType {
    LlmRequest { model, tokens_in, latency_ms },
    LlmResponse { tokens_out, latency_ms, stop_reason },
    ToolExecution { name, latency_ms, status (success/error) },
    CommandExecuted { command, result },
    PermissionCheck { decision (allow/deny/ask) },
    Error { error_type, message },
}
```

**Commands:**
- `/telemetry status` — Show collected events, export status
- `/telemetry export` — Manually trigger OTLP export
- `/telemetry clear` — Clear local event log

**Configuration:**
```toml
[telemetry]
enabled = true
local_path = "~/.oxicode/telemetry/"
otlp_endpoint = "http://localhost:4318"  # OpenTelemetry HTTP endpoint
batch_size = 100
flush_interval_secs = 60
```

**Feature Flag:**
```toml
[features]
telemetry-otlp = ["opentelemetry", "opentelemetry-otlp"]
```

**Dependencies:** opentelemetry, opentelemetry-otlp (OTLP export), serde_json

---

### GitHub Integration (`github/` — included by default)

**Purpose:** GitHub App installation wizard and workflow generation for repository automation

**Architecture:**
```
GitHub App Manifest
  ↓
OxiCode Install Wizard
  ↓
GitHub OAuth Flow
  ↓
Workflow File Generation
  ↓
GitHub Actions Integration
```

**Key components:**
- **`app_installer.rs`** — Guided GitHub App installation (manifest → redirect → OAuth)
- **`workflow_generator.rs`** — Generate `.github/workflows/*.yml` files
- **`github_client.rs`** — GitHub API client (authenticated via App token)
- **`app_config.rs`** — App manifest configuration (permissions, events, webhooks)

**Commands:**
- `/install-github-app [repo]` — Launch guided installer
  - Displays GitHub App manifest URL
  - Waits for OAuth callback (localhost:8080)
  - Generates workflow files in `.github/workflows/`
  - Tests initial workflow execution

**Generated Workflows:**
1. **oxicode-workflow-basic.yml** — Standard OxiCode analysis on PR
2. **oxicode-workflow-advanced.yml** — Multi-model comparison, cost reporting
3. **oxicode-workflow-custom.yml** — User-configured actions

**Workflow Features:**
- Runs OxiCode on PR comments (e.g., `@oxicode-bot analyze`)
- Code quality checks via LLM reasoning
- Automatic commit suggestions
- Status checks integration

**GitHub App Permissions:**
- `contents:read` — Read repo code + workflows
- `pull_requests:read` — Read PR details
- `pull_requests:write` — Create PR comments
- `workflows:write` — Create/update workflow files
- `checks:write` — Report check runs

**Configuration:**
```toml
[github]
app_name = "oxicode"
app_id = "123456"
webhook_secret = "secret"
```

**Features:**
- **Manifest-based Setup** — No manual GitHub App creation
- **Workflow Templates** — Pre-built, customizable workflows
- **Status Checks** — Integrates with GitHub branch protection
- **Comment Triggers** — Listen for `@oxicode-bot` mentions in PR comments

---

## Phase 8: Rewind + Thin Commands

### Conversation Rewind Module (`crates/oxicode-core/src/rewind.rs`)

**Purpose:** Undo recent conversation turns, maintaining consistency across conversation state, token counts, and session storage.

**Architecture:**
```
User → /rewind [N]
         ↓
    RewindRequest { turns: N, guard_streaming: true }
         ↓
    rewind_conversation(conv, N)
         ├── Validate not streaming
         ├── Validate can rewind N turns
         ├── Remove last N turn pairs (user + assistant + tools)
         ├── Update token count
         ├── Update message IDs
         └── Return RewindResult { removed: N, new_length: M }
         ↓
    Save session state to disk
         ↓
    Update TUI with new conversation
```

**Key Functions:**
- `pub fn rewind(conv: &mut Conversation, turns: usize) -> RewindResult` — Remove last N turns
- `fn find_turn_boundaries(messages: &[Message]) -> Vec<(usize, usize)>` — Identify turn pairs
- `fn update_token_metadata(conv: &mut Conversation)` — Recalculate token count post-rewind

**Features:**
- **Turn-boundary Detection:** Correctly identifies user-assistant pairs even with multi-turn tool calls
- **Streaming Guard:** Prevents rewind during active streaming to avoid state corruption
- **Session Persistence:** Automatically saves rewound state to disk
- **Token Recalculation:** Updates conversation token count accurately

**Test Coverage:** 7 tests
- Rewind single turn, rewind multiple turns, rewind past beginning (error), rewind during streaming (error)

**Integration Points:**
- `/rewind` command calls `rewind_conversation()` on active session
- `query_engine` skips turn pairs that were rewound in next iteration
- `StateStore` persists rewound state via `save_session()`

---

### Thin Commands Enhancement

**Enhanced 7 marker-only stub commands with real enforcement:**

#### 1. `/sandbox-toggle` — Shell Tool Restriction
- **Before:** Just toggle a flag, no effect on tool execution
- **Now:** When active, filters `ToolRegistry` to exclude shell tools (bash, powershell, repl)
  - User attempts to use bash → Gets "Shell tools disabled in sandbox mode"
  - All other tools (file_read, web_fetch, etc.) remain available
- **Integration:** `query_engine::check_tool_availability()` checks sandbox state
- **Tests:** 2 tests (enable/disable sandbox, verify tool filtering)

#### 2. `/reload-plugins` — Hot-Reload Plugin Registry
- **Before:** No-op, printed "Reloading plugins..." without actually reloading
- **Now:** Rescan plugin directories, reload manifests, re-register tools
  - Calls `PluginManager::reload_all()`
  - Re-discovers plugins from `~/.oxicode/plugins/`
  - Updates tool descriptions in ToolRegistry
  - Reports: "Reloaded {N} plugins. {M} tools registered."
- **Integration:** Async task spawned from command handler
- **Tests:** 2 tests (discover new plugins, remove stale plugins)

#### 3. `/advisor` — System Prompt Injection
- **Before:** Toggle without behavior change
- **Now:** When active, injects advisor-specific system prompt modifier
  - Base behavior: "Suggest approaches but don't execute tools directly. Ask before acting."
  - Stored in `AppState::advisor_mode`
  - Injected during `build_request()` per-turn
  - LLM changes behavior dynamically based on system prompt
- **Implementation:** `system_prompt::mode_injection_text()` generates mode-specific prompts
- **Tests:** 2 tests (advisor on/off, verify system prompt injection)

#### 4. `/desktop` — Platform-Specific App Launcher
- **Before:** Not implemented
- **Now:** Platform detection + native launcher
  - macOS: `open -a OxiCode` or `open .` (Finder)
  - Linux: `xdg-open .` (default file manager)
  - Windows: `explorer .` (File Explorer)
  - Fallback: Error message for unsupported platforms
- **Integration:** Desktop command spawns subprocess, returns status
- **Tests:** Platform-specific conditional tests

#### 5. `/rate-limit-options` — Real Rate Limit State Display
- **Before:** Printed dummy values
- **Now:** Displays real state from Phase 2 (`RateLimitState`)
  - Current usage: {requests_used}/{requests_limit}
  - Tokens used: {token_count}/{token_limit}
  - Reset time: {UTC timestamp}
  - Config options: display available overrides
- **Integration:** Reads from `AppState::rate_limit_state` (Phase 2)
- **Tests:** 2 tests (display current state, handle rate limit exceeded)

#### 6. `/output-style` — Style Directory Scanning
- **Before:** Not implemented
- **Now:** Scan `~/.oxicode/styles/` for TOML style files
  - User selects style from available options
  - Load style config, apply to output formatting
  - Example styles: "dark", "light", "monochrome", "hacker"
- **Integration:** Style applied during TUI rendering
- **Tests:** 2 tests (scan directory, apply style)

#### 7. `/voice (partial)` — Enhanced Voice Control
- **Before:** Simple on/off toggle
- **Now:** Real voice capture with speech-to-text integration (Phase 7)
  - Microphone status indicator in status bar (🎤 green/yellow/red)
  - VAD (Voice Activity Detection) for silence handling
  - Whisper API integration for transcription
- **Tests:** Integrated with Phase 7 voice tests

---

### System Prompt Mode Injection (`crates/oxicode-core/src/system_prompt.rs`)

**New Function:** `pub fn mode_injection_text(modes: &[AppMode]) -> String`
- Generates mode-specific system prompt modifiers
- Supports: advisor, sandbox, strict
- Example output:
  ```
  [ADVISOR MODE]
  You are in advisor mode. Suggest approaches but don't execute tools directly.
  Ask the user for approval before taking action.
  ```

**Integration into QueryEngine:**
- `build_request()` now calls `mode_injection_text()` for active modes
- Injects modes into system prompt AFTER tool schemas, BEFORE conversation
- Per-turn injection allows dynamic mode toggling mid-conversation

**Test Coverage:** 8 new tests
- Single mode injection, multiple modes, no modes (empty)
- Verify mode text appears in final system prompt

---

### Summary of Changes

**New Module:**
- `crates/oxicode-core/src/rewind.rs` (~130 LOC, 7 tests)

**Modified Modules:**
- `crates/oxicode-core/src/lib.rs` — Added `pub mod rewind;`
- `crates/oxicode-core/src/system_prompt.rs` — Added `mode_injection_text()`, `assemble_system_prompt_with_modes()` (8 tests)
- `crates/oxicode-core/src/query_engine.rs` — Dynamic mode injection per-turn
- `crates/oxicode-cli/src/commands/new_commands.rs` — Enhanced `/rewind`, `/sandbox-toggle`
- `crates/oxicode-cli/src/commands/info_commands.rs` — Enhanced `/rate-limit-options`, `/advisor`, `/reload-plugins`
- `crates/oxicode-cli/src/commands/plugin_commands.rs` — Removed duplicate registration
- `crates/oxicode-cli/src/commands/mod.rs` — Cleaned up command routing

**Test Results:**
- 1,037 workspace tests, 0 failures
- 15 new tests (7 rewind + 8 system_prompt)

---

## Cargo Features

**Feature combinations for Phase 7:**

```toml
[features]
default = ["bridge"]
voice = ["cpal", "hound"]
bridge = ["tokio-tungstenite", "jsonwebtoken"]
telemetry-otlp = ["opentelemetry", "opentelemetry-otlp"]
github = []  # Included by default
full = ["voice", "bridge", "telemetry-otlp"]  # All Phase 7 features
```

**Build commands:**
```bash
cargo build --features voice                    # Voice only
cargo build --features bridge                   # Bridge only
cargo build --features telemetry-otlp           # Telemetry only
cargo build --features full                     # All Phase 7
cargo build --all-features                      # Everything
```

---

## Phase 9: Test Coverage Push

### Overview

Expanded test coverage from 838 tests (776 effective) to 1,132 tests (+294 new tests), achieving >80% coverage on core crates. Introduced `MockLlmProvider` for deterministic integration testing.

**Key Metrics:**
| Crate | Before | After | Growth |
|-------|--------|-------|--------|
| oxicode-core | 26 | 53 | +27 tests |
| oxicode-hooks | 11 | 47 | +36 tests |
| oxicode-session | 43 | 59 | +16 tests |
| oxicode-tools | 91 | 167 | +76 tests |
| oxicode-api | 96 | 103 | +7 tests |
| **Total** | **838** | **1,132** | **+294 tests** |

---

### MockLlmProvider (`crates/oxicode-api/src/mock.rs`)

**Purpose:** Configurable mock LLM for deterministic testing — eliminates external API dependencies.

**Features:**
- **Predefined Responses:** Configure text, tool_use, stop_reason per test
- **Streaming Simulation:** Simulates streaming behavior without actual network calls
- **Configurable Metadata:** Model name, token counts, stop reasons
- **`#[cfg(test)]` Gated:** Zero overhead in production builds

**Architecture:**
```
MockLlmProvider {
  responses: Vec<LlmResponse>,
  config: MockConfig,
}
  ↓
configure_response(text, tool_uses, stop_reason)
  ↓
stream_message() → Returns next response
```

**Usage Example:**
```rust
let mock = MockLlmProvider::new()
  .with_response("Hello, world!")
  .with_tool_use("bash", {"command": "ls"})
  .with_stop_reason(StopReason::ToolUse);

let result = mock.stream_message(query).await?;
```

**Test Coverage:**
- Single-turn text response
- Multi-turn streaming
- Tool use with results
- Error handling (API errors, timeouts)

---

### Test Suite Expansion

#### oxicode-core (27 new tests)
- **Query Engine:** Multi-turn execution, tool dispatch, budget enforcement, stop reason handling
- **System Prompt:** Mode injection (advisor/sandbox/strict), component assembly

#### oxicode-hooks (36 new tests)
- All 26 hook event types fire correctly
- Hook response types (Pass, ModifyPrompt, OverrideResult, Abort)
- Timeout enforcement (10s limit)
- Config loading from settings

#### oxicode-session (16 new tests)
- Memdir path resolution with git roots
- MEMORY.md creation and truncation (200-line limit)
- Memory scanner: file sorting, capping, frontmatter parsing
- Session save/load roundtrip

#### oxicode-tools (76 new tests)
- Per-tool: happy path + error scenarios
- BashTool: 30+ dangerous command patterns detected
- FileEdit: uniqueness checks, replace_all mode
- GrepTool: regex, context lines, file type filtering

#### oxicode-api (7 new tests)
- MockLlmProvider configuration
- Streaming behavior
- Response chaining

#### Integration Tests (50+ new tests)
- Full multi-turn conversation flows
- Permission pipeline: request → approval/denial → execution
- Session persistence: converse → save → load → continue
- Cost tracking accumulation
- Rate limiting state machine

---

## Gap Closure (Phases 7-10): Advanced Features

### Phase 7: AutoDream Suggestion Engine

**Purpose:** Context-aware action suggestions based on conversation patterns.

**Architecture:**
```
Conversation history
    ↓
Pattern matcher (LSTM-style context aggregation)
    ↓
AutoDreamService::suggest_next_action(context)
    ↓
Returns top 3 actions: ["run tests", "commit changes", "review PR"]
```

**Key Components:**
- `AutoDreamConfig` — Maintains context aggregator + pattern database
- `AutoDreamService` — Inference engine (heuristic pattern matching)
- Context includes: recent messages, tool history, current mode (advisor/sandbox/strict)

**Integration:** Injected into system prompt as `next_action_suggestions: [...]`

---

### Phase 7: Session Recording & Replay (VCR)

**Purpose:** Record and replay full conversation sessions for debugging and analysis.

**Architecture:**
```
QueryEngine.execute_turn()
    ↓
VcrRecorder::record_session(messages)
    ↓
Serialize to gzip JSON: ~/.oxicode/vcr/{session_id}.vcr.gz
    ↓
On `/vcr play {session_id}`:
    ↓
VcrPlayer::replay(path) → Deserialize + reconstitute messages
    ↓
Re-run query loop with same inputs/outputs
```

**Storage Format (gzip'd JSONL):**
```json
{"message_index":0,"role":"user","content":"hello","created_at":"2026-04-05T12:00:00Z"}
{"message_index":1,"role":"assistant","content":"Hi there!","tool_uses":[]}
{"message_index":2,"role":"user","content_type":"tool_result","tool_use_id":"x","result":"..."}
```

**Key Components:**
- `VcrRecorder` — Capture messages + metadata
- `VcrPlayer` — Playback state machine (Init → Playing → Done)
- `VcrStorage` — File I/O with gzip compression

**Commands:**
- `/vcr record [session_id]` — Start/stop recording
- `/vcr play [session_id]` — Replay session
- `/vcr list` — List recorded sessions

---

### Phase 7: Performance Metrics & Bridge Diagnostics

**Purpose:** Track latency percentiles (p50, p95) for debugging bridge/network issues.

**Architecture:**
```
BridgeHealthCheck (10s interval)
    ↓
Ping/pong exchange
    ↓
Measure latency
    ↓
BridgeDiagnostics::record_latency(message_type, duration_ms)
    ↓
Update histogram: {p50, p95, max, count}
```

**Key Components:**
- `PerfMetrics` — Percentile calculation (sorted array approach)
- `BridgeDiagnostics` — Per-message-type latency tracking
- `BridgeHealthCheck` — Ping/pong with exponential backoff on failure
- `BridgeStatusTracker` — Connection state machine (Connected/Connecting/Reconnecting/Disconnected)

**Output Format:**
```json
{
  "message_type": "tool.result",
  "latency_ms": {"p50": 45, "p95": 120, "max": 250},
  "success_rate": 0.98,
  "error_count": 2
}
```

---

### Phase 7: Bridge Debug Logging & Message Inspection

**Purpose:** Debug bridge protocol issues with ring-buffer logging and auth redaction.

**Architecture:**
```
BridgeDebugLogger (feature-gated: bridge_debug)
    ↓
Ring buffer (last 100 messages, ~50KB memory)
    ↓
On error or manual export:
    ↓
BridgeMessageInspector::format_message(msg) → Pretty-print with auth token redaction
    ↓
Save to ~/.oxicode/bridge-debug.log (10MB rotation)
```

**Key Components:**
- `BridgeDebugLogger` — Ring-buffer storage (thread-safe Arc<Mutex>)
- `BridgeMessageInspector` — Message formatting + redaction (replaces tokens with `***`)
- `BridgeEventTap` — tokio::broadcast event subscriptions for real-time monitoring

**Feature Gate:** `bridge_debug` (optional, off by default)

---

### Phase 7: DNS Pinning (TOCTOU Protection)

**Purpose:** Prevent time-of-check-time-of-use (TOCTOU) attacks in HTTP hook execution.

**Architecture:**
```
Hook executor needs to POST to https://example.com:
    ↓
PinnedResolver::resolve("example.com")
    ↓
DNS lookup → 93.184.216.34
    ↓
Check: Is 93.184.216.34 private? (reject 127.0.0.1, 10.0.0.0/8, etc)
    ↓
Cache for 30 seconds (TTL = 30s)
    ↓
Execute POST to 93.184.216.34 (NOT dynamic re-resolve)
```

**Private IP Rejection List:**
- 127.0.0.0/8 (localhost)
- ::1 (IPv6 loopback)
- 10.0.0.0/8 (private)
- 172.16.0.0/12 (private)
- 192.168.0.0/16 (private)
- fe80::/10 (IPv6 link-local)
- fc00::/7 (IPv6 unique local)

**Implementation:** `oxicode-hooks/src/pinned_resolver.rs`

---

### Phase 8-10: CLI Commands & UI Stubs

**New Commands (5):**
- `/ultraplan` — Generate ultra-concise project plans
- `/buddy` — Casual conversation mode
- `/good_claude` — Self-assessment + quality improvements
- `/settings_sync` — Sync settings with remote endpoint
- `/vcr [record|play|list]` — Session recording/playback

**UI Stubs (3, oxicode-mcp bridge layer):**
- `BridgeUiPermissionDialog` — Permission request dialog (stub for future TUI integration)
- `BridgeUiConfigDialog` — Bridge configuration dialog (stub)
- `BridgeUiNotification` — Bridge status notification (stub)

---

### Test Coverage Summary (Gap Closure)

**Test Expansion:** 756 → 1,143 tests (+387 new tests)

**By Phase:**
- Phase 7 (Core/VCR/AutoDream): 70+ unit tests
- Phase 8 (Commands): 40+ unit tests
- Phase 9 (Coverage push): 150+ unit + integration tests
- Phase 10 (Integration & smoke): 127+ integration tests

**Coverage Goals Met:**
- ✓ All gap-closure modules ≥70% coverage
- ✓ AutoDream: 8 test cases (context aggregation, pattern matching fallback)
- ✓ VCR: 12 test cases (record, replay, gzip roundtrip)
- ✓ PerfMetrics: 6 test cases (percentile calculation, edge cases)
- ✓ Bridge diagnostics: 15+ test cases (health check, latency tracking, state machine)
- ✓ DNS pinning: 8 test cases (private IP rejection, cache TTL, concurrency)
- ✓ CLI commands: 5+ test cases (command parsing, result formatting)

---


1. **Phase 8 (Rewind + Thin Commands):** Conversation rewind, thin command enforcement (COMPLETE ✓)
2. **Phase 9 (Test Coverage Push):** Comprehensive test coverage across all modules
3. **Phase 10 (Integration & Smoke Testing):** End-to-end testing and performance optimization

