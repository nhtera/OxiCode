# OxiCode — System Architecture

**Version:** 0.2.0 | **Last Updated:** 2026-04-03 | **Phase:** 4 (Multi-Agent, Skills, Context Defense) + Phase 2 API (Multi-Provider)

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
│  │ - 33 built-in tools (file, bash, grep, agent, etc)  │ │
│  │ - Phase 1: MCP resource tools, skill invocation      │ │
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

## Next Steps (Phase 5)

1. **User Commands:** `/agent list`, `/agent send`, `/skills list`, `/task view`
2. **Team UI:** Split pane for agent monitoring, skill activation UI
3. **Marketplace:** Register skills online, version management
4. **Advanced Compaction:** Priority-based (recent messages higher priority), context pruning strategies
5. **Observability:** Metrics dashboard, debug mode, trace export

