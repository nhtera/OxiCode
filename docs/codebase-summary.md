# OxiCode — Codebase Summary

**Version:** 0.5.1 | **Last Updated:** 2026-04-05 | **Scope:** Gap Closure (Phases 7-10) Complete + 24 New Modules | **Total:** 20 crates, 46 oxicode-tools files, 170K tokens

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
    │     PHASE 5: PLUGINS & ENTERPRISE LAYER          │
    │                                                   │
    │  oxicode-plugins (Registry, Trust, Hot-Reload)  │
    │  oxicode-config enhanced (Enterprise Settings)   │
    └───────────┬────────────────────────────────────┘
                │
    ┌───────────┴──────────────────────────────────┐
    │    PHASE 7: VOICE, BRIDGE, TELEMETRY, GITHUB  │
    │                                               │
    │  oxicode-voice ─ oxicode-remote ── oxicode-  │
    │  (Whisper)       (WebSocket)      telemetry  │
    │                                   (OTLP)     │
    │                                               │
    │  oxicode-github (App, Workflows)             │
    └───────────┬──────────────────────────────────┘
                │
    ┌───────────┴──────────────────────────────────┐
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

#### oxicode-api (~900 LOC, Gap Closure +300 LOC)
**Purpose:** LLM provider traits + message types + streaming + multipart uploads + policy limits

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
- **Gap Closure NEW:**
  - `MultipartUploader` — Multipart file upload with MIME detection
  - `PolicyLimitsPoller` — ETag-cached policy limits polling (rate limit aware)

**Files:**
- provider.rs (trait definition)
- anthropic.rs (~800 LOC, most complex)
- openai.rs
- compatible.rs
- mcp.rs
- **files_multipart.rs** (Multipart form parser + MIME detection)
- **policy_limits_poller.rs** (ETag-based caching, 5min TTL)

**Used by:** Core (QueryEngine), TUI (streaming display), Tools (file upload)

**Quality:** Stream error handling, SSE parsing, retry logic, MIME validation

**Top file:** openai_compatible.rs (3,681 tokens, 3.6% of codebase)

---

### Layer 2: Core Execution

#### oxicode-core (~1,200 LOC, Gap Closure +800 LOC)
**Purpose:** Main query execution loop, LLM orchestration, suggestion engine, session recording

**Key exports:**
- `QueryEngine` struct (main loop: stream → extract tools → execute → recurse)
- `assemble_system_prompt()` — compose system message from multiple sources
- `MAX_TOOL_TURNS = 50` (prevent infinite loops)
- **Gap Closure NEW:**
  - `AutoDreamService` — Context-aware suggestion engine (LSTM-style pattern matching)
  - `VcrRecorder` — Session recording with gzip JSON serialization
  - `VcrPlayer` — Session replay + diagnostics
  - `PerfMetrics` — Latency tracking with p50/p95 percentiles
  - `DiagnosticTracker` — Opt-in telemetry for debugging
  - `ThinkingBlockStore` — Bounded VecDeque for thinking history

**Files:**
- query_engine.rs (main loop)
- system_prompt.rs (prompt assembly)
- message_builder.rs (helper)
- **auto_dream_config.rs** (Context aggregator, pattern DB)
- **auto_dream_service.rs** (Suggestion inference engine)
- **vcr_recorder.rs** (Session recording + compression)
- **vcr_player.rs** (Session playback state machine)
- **vcr_storage.rs** (VCR file I/O, gzip JSON)
- **perf_metrics.rs** (Latency tracking + percentiles)
- **diagnostic_tracker.rs** (Feature-gated telemetry)
- **thinking_block_store.rs** (Circular buffer for thinking blocks)

**Key methods:**
```rust
pub async fn execute_turn(&self, conversation: &mut Conversation) -> OxiResult<Message>
pub fn suggest_next_action(&self, context: &SuggestionContext) -> Vec<String>
pub fn record_session(&mut self, messages: &[Message]) -> OxiResult<()>
pub async fn replay_session(&self, vcr_path: &Path) -> OxiResult<Vec<Message>>
```

**Used by:** CLI, TUI, Server mode

**Quality:** No panics, proper error propagation, tool execution hooks, feature-gated telemetry

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

#### oxicode-session (~1,000 LOC)
**Purpose:** Session persistence (JSONL) + memory extraction, selection, and team sync (Phase 2)

**Core modules:**
- `memory.rs` — Pattern-based memory extraction from text
- `memory_types.rs` — MemoryEntry, MemoryType enum (Decision, Preference, Context, etc)
- `memory_scanner.rs` — Scan project memdir for memories
- **`memory_selector.rs` (NEW)** — LLM-based relevance selection (top 5 from 200)
- **`memory_extractor.rs` (NEW)** — Auto-extract at session end, max 10 per session
- **`memory_freshness.rs` (NEW)** — Append age caveats ("X days ago — verify")
- **`team_memory_sync.rs` (NEW)** — Delta-sync with SHA-256 checksums (feature: `team_memory_sync`)

**Key exports:**
- `SessionManager` (load, save, append)
- `MemoryEntry`, `MemoryType` (persistence)
- `MemorySelector::select_relevant()` (LLM-based filtering)
- `MemoryExtractor::extract_and_save()` (auto-extraction)
- `freshness_warning()` (age annotation)
- `TeamMemorySync::sync()` (feature-gated team sync)

**Key methods:**
```rust
pub fn load(session_dir: &Path) -> OxiResult<Session>;
pub fn save(&self, path: &Path) -> OxiResult<()>;
pub async fn select_relevant(llm, query, memories, max) -> SelectionResult;
pub fn extract_and_save(messages, session_id) -> ExtractionResult;
pub fn freshness_warning(created_at) -> Option<String>;
```

**Used by:** CLI (load/save), TUI (persist + memory injection), QueryEngine (system prompt)

**Quality:** Atomic writes, pattern extraction, LLM fallbacks, comprehensive tests (59 tests)

---

#### oxicode-hooks (~1,200 LOC, Gap Closure +150 LOC)
**Purpose:** 29 lifecycle event hooks with 3 execution modes (command, agent, HTTP), DNS pinning for TOCTOU protection

**Key exports:**
- `HookManager` (fire events, dispatch to executors)
- `HookEvent` enum (29 events: core + extended + rate limit + cost)
- `HookType` enum (Command | Agent | Http) — default Command for backward compat
- `HookDef` (per-event config with type-specific settings)
- `HookPayload` / `HookResponse` (Pass | ModifyPrompt | Abort | OverrideResult)
- **Gap Closure NEW:**
  - `PinnedResolver` — DNS-pinned resolver with 30s TTL cache + private IP rejection

**Modules:**
- `config.rs` — TOML parsing, supports string + table forms, inline + nested agent/http config
- `events.rs` — 29 event definitions with serialization
- `executor.rs` — 3-way dispatch: shell subprocess, agent LLM call, HTTP POST
- `manager.rs` — Central coordinator, session/model context injection
- `agent_hook_executor.rs` — LLM-based hooks with structured output parsing, 60s timeout
- `http_hook_executor.rs` — HTTP POST hooks with SSRF guard (private IP rejection), 10min timeout
- **pinned_resolver.rs** (DNS cache + TOCTOU protection, rejects 127.0.0.1, ::1, 192.168.0.0/16, etc)

**DNS Pinning Features:**
- 30-second TTL cache for resolved addresses
- SSRF protection: reject private IP ranges (127.0.0.1, ::1, 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, fc00::/7)
- Prevent time-of-check-time-of-use attacks
- Configurable cache size (default: 1000 entries)

**Used by:** QueryEngine (hook points), HTTP hook executor (SSRF guard), CLI (user-defined hooks)

**Quality:** Fail-open design (timeout/error → Pass), comprehensive SSRF protection, DNS cache validation, 94 tests

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

### Layer 5: Phase 5 — Plugin Marketplace & Enterprise Settings

#### oxicode-plugins (~2.1K LOC, new crate)
**Purpose:** Remote plugin registry client, trust assessment, hot-reload, lifecycle management

**Key exports:**
- `PluginRegistry` — Fetch remote index, cache, search/filter by keywords/version
- `PluginManager` — Discover → validate → install → hot-reload workflow
- `PluginEntry` — name, version, download_url, trust (verified/community/unverified), permissions
- `TrustLevel` enum — Verified (signed), Community (voted), Unverified (default)
- `PluginManifest` — Plugin.toml: name, version, permissions, min_oxicode_version

**Files:**
- registry.rs — Remote index fetch, caching (TTL: 1 hour), search/filter APIs
- manager.rs — Plugin lifecycle, validation, discovery
- security.rs — Trust assessment, permission validation
- manifest.rs — Plugin.toml parsing
- install.rs — Download tar.gz, extract, install flow
- lifecycle.rs — Enable/disable state machine
- subprocess.rs — Subprocess spawning (sandbox-ready)

**Key methods:**
```rust
pub async fn fetch_index(url: &str) -> OxiResult<Vec<PluginEntry>>;
pub fn search(query: &str, entries: &[PluginEntry]) -> Vec<PluginEntry>;
pub async fn install(entry: &PluginEntry) -> OxiResult<()>;
pub fn reload_plugins(&mut self) -> OxiResult<()>;
```

**Commands (CLI integration via oxicode-cli/commands/plugin_commands.rs):**
- `/plugin browse [--category TAG]` — List marketplace
- `/plugin search QUERY` — Search by keywords
- `/plugin info NAME [--version V]` — Details + trust level
- `/plugin install NAME[@VERSION]` — Download + install
- `/plugin update [NAME]` — Update all or specific
- `/plugin remove NAME` — Uninstall
- `/plugin list [--status]` — Local installed
- `/reload-plugins` — Hot-reload without restart

**Used by:** CLI (plugin commands), ToolRegistry (plugin tools), Manager (lifecycle)

**Dependencies:** reqwest, flate2, tar, serde_json, chrono, tokio

---

#### oxicode-config enhancement — Enterprise Settings
**Purpose:** Remote admin endpoint for enterprise settings with HMAC-SHA256 validation, cloud sync

**New files:**
- enterprise_settings.rs (~400 LOC) — EnterpriseSettingsClient, caching, signature validation
- Updated settings.rs — Merge enterprise + local settings, conflict detection

**Key types:**
```rust
pub struct EnterpriseSettingsClient {
    endpoint: String,           // OXICODE_ENTERPRISE_SETTINGS_URL
    signing_key: Option<String>, // OXICODE_ENTERPRISE_KEY
    cache_dir: PathBuf,
    cache_ttl_secs: i64,        // Default: 3600
}

pub struct EnterpriseSettingsResponse {
    settings: HashMap<String, String>,
    locked: HashMap<String, bool>,  // Immutable keys
    signature: String,              // HMAC-SHA256 (hex)
    version_ts: Option<String>,     // Admin version
}
```

**Key methods:**
```rust
pub async fn fetch_settings(&self, oauth_token: Option<&str>) -> OxiResult<EnterpriseSettingsResponse>;
pub async fn push_settings(&self, settings: &HashMap<String, String>, oauth_token: &str) -> OxiResult<()>;
pub async fn pull_settings(&self, oauth_token: &str) -> OxiResult<HashMap<String, String>>;
pub fn sync_status(&self) -> OxiResult<SyncStatus>;
```

**Validation flow:**
1. Fetch settings from remote endpoint
2. Extract signature from response
3. Compute HMAC-SHA256(signing_key, payload)
4. Compare signatures (hex match)
5. Cache locally if valid
6. Merge with user local settings (latest-wins)

**Commands (CLI integration via oxicode-cli/commands/enterprise_commands.rs):**
- `/enterprise pull` — Fetch admin settings
- `/enterprise push` — Upload user settings
- `/enterprise status` — Sync status + conflict report
- `/enterprise diff` — Show changes vs. local

**Used by:** Config loader, Settings manager, CLI commands

**Dependencies:** hmac, sha2, hex, reqwest (enhanced), chrono (enhanced)

---

### Layer 7: Phase 7 — Voice, Bridge, Telemetry & GitHub

#### oxicode-voice (~400 LOC, feature-gated: `voice`)
**Purpose:** Real-time voice capture, speech-to-text via Whisper API

**Key exports:**
- `AudioCapture` — cpal microphone management
- `VoiceActivityDetector` — Silence detection + noise filtering
- `WhisperClient` — Async Whisper API client with retry logic
- `VoiceCommandParser` — Parse transcribed text as commands/input

**Files:**
- audio_capture.rs — PCM buffer handling, device enumeration
- vad.rs — Voice activity detection
- whisper_client.rs — API client + streaming response parsing
- voice_command_parser.rs — Command/text parsing from transcription

**Commands:**
- `/voice on|off|status` — Control voice input

**TUI Integration:**
- Status bar shows 🎤 with color state (green/yellow/red)

**Feature Flag:** `voice` (requires cpal, hound)

**Used by:** TUI (voice input events), QueryEngine (message assembly)

---

#### oxicode-remote (~420 LOC, feature-gated: `bridge`)
**Purpose:** WebSocket bridge for multi-device session control

**Key exports:**
- `BridgeServer` — WebSocket server, session routing
- `BridgeClient` — Client connection to remote endpoint
- `SessionPool` — Multiplexed session management with JWT auth
- `MessageRelay` — Relay QueryEngine messages between endpoints
- `JwtAuth` — Token generation + validation

**Files:**
- bridge_server.rs — WebSocket listener, session pool
- bridge_client.rs — Client to connect to remote bridge
- session_pool.rs — JWT validation, session routing
- message_relay.rs — Message forwarding logic
- auth.rs — JWT token handling
- error.rs — Error types for bridge ops

**Commands:**
- `/remote-setup` — Configure bridge endpoint
- `/bridge [port]` — Start local bridge server
- `/remote-env [list|set KEY VALUE]` — Manage remote environment

**Features:**
- Session pool with separate JWT tokens per session
- Message relay for tool results, state updates
- Automatic reconnect on timeout
- Remote environment variable sync

**Feature Flag:** `bridge` (requires tokio-tungstenite, jsonwebtoken)

**Used by:** CLI (/remote-* commands), QueryEngine (message relay)

---

#### oxicode-telemetry (~380 LOC, feature-gated: `telemetry-otlp`)
**Purpose:** Structured event collection + OTLP export

**Key exports:**
- `TelemetryCollector` — Event buffering, batching
- `TelemetryEvent` — Event schema (type, timestamp, metadata)
- `NdjsonLogger` — Local persistence (NDJSON + rotation)
- `OtlpExporter` — OpenTelemetry Protocol HTTP export
- `MetricsAggregator` — Counters, histograms, gauges

**Files:**
- collector.rs — Event collection + batching
- event.rs — TelemetryEvent schema definition
- ndjson_logger.rs — Local NDJSON persistence with rotation
- otlp_exporter.rs — OTLP HTTP export (configurable endpoint)
- metrics.rs — Metric aggregation

**Event Types:**
- LlmRequest (model, input tokens, latency)
- LlmResponse (output tokens, latency, stop_reason)
- ToolExecution (name, latency, status)
- CommandExecuted (command, result)
- PermissionCheck (decision: allow/deny/ask)
- Error (error_type, message)

**Commands:**
- `/telemetry status` — Show collected events
- `/telemetry export` — Manually trigger OTLP export
- `/telemetry clear` — Clear local event log

**Configuration:**
```toml
[telemetry]
enabled = true
local_path = "~/.oxicode/telemetry/"
otlp_endpoint = "http://localhost:4318"
batch_size = 100
flush_interval_secs = 60
```

**Feature Flag:** `telemetry-otlp` (requires opentelemetry, opentelemetry-otlp)

**Used by:** QueryEngine (LLM/tool event hooks), TUI (command events), CLI (status)

---

#### oxicode-github (~380 LOC, no feature gate)
**Purpose:** GitHub App installation + workflow generation

**Key exports:**
- `AppInstaller` — Guided GitHub App setup wizard
- `WorkflowGenerator` — Generate `.github/workflows/*.yml` files
- `GitHubClient` — GitHub API client (authenticated)
- `AppConfig` — App manifest configuration
- `WorkflowTemplate` — Workflow YAML builder

**Files:**
- app_installer.rs — Installation flow (manifest → OAuth → callback)
- workflow_generator.rs — Generate workflow YAML files
- github_client.rs — GitHub API wrapper (repos, workflows, PRs)
- app_config.rs — App manifest + permissions
- workflow_templates.rs — Pre-built workflow templates
- error.rs — GitHub API error handling

**Commands:**
- `/install-github-app [repo]` — Guided GitHub App installer
  - Displays GitHub App manifest URL
  - Handles OAuth callback (localhost:8080)
  - Generates `.github/workflows/*.yml`
  - Tests initial workflow

**Generated Workflows:**
1. oxicode-workflow-basic.yml — Standard analysis on PR
2. oxicode-workflow-advanced.yml — Multi-model, cost reporting
3. oxicode-workflow-custom.yml — User-configured actions

**GitHub App Permissions:**
- contents:read — Read code + workflows
- pull_requests:read — Read PR details
- pull_requests:write — Create PR comments
- workflows:write — Create/update workflows
- checks:write — Report check runs

**Features:**
- Manifest-based setup (no manual GitHub App creation)
- Pre-built workflow templates (customizable)
- GitHub status checks integration
- `@oxicode-bot` comment triggers

**Used by:** CLI (/install-github-app command), workflow automation

---

#### oxicode-mcp (~1,050 LOC, Gap Closure +800 LOC)
**Purpose:** MCP (Model Context Protocol) client & server, bridge debugging, diagnostics, health checks

**Key exports:**
- `McpServerManager` — manages multiple MCP client connections (stdio, HTTP/SSE transports)
- `list_mcp_tools()`, `call_mcp_tool()` — query external MCP tool catalog & invoke
- `list_mcp_resources()`, `read_mcp_resource()` — access MCP resource URIs
- `list_prompts()`, `get_prompt()` — query MCP prompt catalog
- `OxiMcpServer` + `OxiMcpServerBuilder` — expose OxiCode as MCP server to external clients
- Type bridge: `mcp_tool_to_schema()`, `mcp_resource_to_toolschema()`
- **Gap Closure NEW (9 modules):**
  - `BridgeDebugLogger` — Ring-buffer message logger (feature-gated: bridge_debug)
  - `BridgeStatusTracker` — Connection state machine (Connected/Connecting/Reconnecting/Disconnected)
  - `BridgeDiagnostics` — Per-type latency tracking (p50/p95)
  - `BridgeMessageInspector` — Message formatting + auth redaction
  - `BridgeHealthCheck` — Ping/pong health checker (10s interval)
  - `BridgeEventTap` — tokio::broadcast event tap for debugging
  - `BridgeUiPermissionDialog` — UI stub for permission flows
  - `BridgeUiConfigDialog` — UI stub for bridge configuration
  - `BridgeUiNotification` — UI stub for bridge notifications

**Transport support (via rmcp):**
- Stdio (child process JSON-RPC)
- HTTP + SSE (streaming HTTP for WebSocket-like semantics)

**Files:**
- manager.rs — McpServerManager lifecycle (spawn, shutdown, multi-client storage)
- server.rs — OxiMcpServer impl, ServerHandler trait for tool/resource/prompt dispatch
- config.rs — McpTransportType enum (Stdio, Http), TOML parsing
- types.rs — Type bridges between rmcp and oxicode-tools models
- doctor.rs — Health checks for MCP connections
- **bridge_debug_logger.rs** (Ring buffer, optional feature)
- **bridge_status_tracker.rs** (Connection state machine)
- **bridge_diagnostics.rs** (Latency metrics per message type)
- **bridge_message_inspector.rs** (Message formatting + redaction)
- **bridge_health_check.rs** (Ping/pong + keepalive)
- **bridge_event_tap.rs** (tokio::broadcast event logging)
- **bridge_ui_permission_dialog.rs** (UI stub)
- **bridge_ui_config_dialog.rs** (UI stub)
- **bridge_ui_notification.rs** (UI stub)

**Features (Phase 2):**
- Prompts API: query external server prompts + parameters
- Roots declaration: declare resources owned by client
- Streamable HTTP: alias for HTTP/SSE transport type

**Server mode (Phase 3):**
- `oxicode --mcp` spawns server listening on stdio
- Dynamic tool dispatch via ToolRegistry
- Demo read_file tool handler

**Gap Closure Features:**
- Per-message latency tracking (histogram + percentile)
- Real-time connection state visibility
- Message logging with auth token redaction
- Health check with exponential backoff on failure
- Event broadcasting for debugging

**Used by:** ToolRegistry (external tool bridging), oxicode-core (MCP queries), CLI (mcp-servers, mcp-connect commands), Bridge diagnostics

**Dependency:** rmcp v1.3+ (Rust MCP SDK)

**Quality:** Zero unsafe code, comprehensive tests (17 total), bridge debug feature-gated, zero clippy warnings

---

#### oxicode-tui (~800 LOC)
**Purpose:** Terminal UI (ratatui), event loop, rendering, vim mode

**Key exports:**
- `App` — main TUI struct (state, input, rendering)
- `Renderer` — draw functions for each component
- Event loop (keyboard → UiEvent → core → StateStore → redraw)
- Widgets: MessageView, InputBox, StatusBar, AgentPanel, TaskPanel, NotificationPanel
- **Phase 6 NEW:** VimTextObjects (iw/aw, i"/a", i(/a(, i{/a{, operators: diw, ci", ya{)

**Top file:** tui/app.rs (2,456 tokens, 2.4% of codebase)

**Phase 6 additions:**
- `vim_mode.rs` — Vim text object support (iw, aw, i", a", i(, a(, i{, a{ with operators)
- `vim_text_objects.rs` (NEW) — Text object parsing & execution

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

#### oxicode-cli (~650 LOC, Gap Closure +250 LOC)
**Purpose:** Slash commands, REPL, CLI parsing, gap-closure commands

**Key exports:**
- `CommandRegistry` — HashMap of slash commands
- `SlashCommand` trait (execute, completions)
- `CommandOutput` enum (Message, Silent, Quit, Error)
- **Built-in commands: 96 total (was 91 in Phase 6)**
  - Core: help, version, clear, status, quit, config, model, permissions
  - Session: save, load, export, undo, redo, rename, resume
  - Git: commit, pr, branch, log, stash, push, pull (7 Phase 5)
  - Debug: compact, context, usage, tokens, doctor
  - MCP: mcp-servers, mcp-tools, mcp-connect, mcp-disconnect
  - Team/Agents: team, agents, task, plugin, plan
  - View: theme, shortcuts, about, tools, history, vim, review (+ many workflow stubs)
  - **Gap Closure NEW (5 commands):** ultraplan, buddy, good_claude, settings_sync, vcr
  - **File management:** view_commands.rs split into 3 modules (each <200 lines)

**Gap Closure new commands:**
- `/ultraplan` — Generate ultra-concise project plans
- `/buddy` — AI buddy mode (casual conversation)
- `/good_claude` — Self-assessment + quality improvements
- `/settings_sync` — Sync settings with remote endpoint
- `/vcr [record|play|list]` — Session recording/playback

**Files:**
- commands/mod.rs (registry)
- commands/git_commands.rs + git_helpers.rs (Phase 5)
- commands/session_commands.rs (Phase 5: undo, rename)
- commands/provider.rs (permissions)
- commands/mcp_commands.rs, team_commands.rs, task_commands.rs, plugin_commands.rs, plan_commands.rs
- commands/debug_commands.rs (Phase 5: usage, context enhancements)
- commands/general.rs (vim)
- commands/view_commands.rs, workflow_commands.rs, session_view_commands.rs (Phase 5 split)
- **commands/ultraplan_command.rs** (Ultra-concise planning)
- **commands/buddy_command.rs** (Casual chat mode)
- **commands/good_claude_command.rs** (Quality assessment)
- **commands/settings_sync_command.rs** (Remote sync)
- **commands/vcr_command.rs** (Session recording)
- repl.rs (REPL mode with readline)

**Used by:** TUI (command parsing), CLI (REPL), Server mode

**Quality:** Good error messages, help text, command completions, feature-gated VCR

---

## Code Statistics

| Metric | Value |
|--------|-------|
| Total Crates | 20 |
| Total Files | 179 (was 155 in Phase 6, +24 gap-closure) |
| Total Tokens | 170,000 (was 156,000) |
| Total Chars | 620,000 (was 580,000) |
| LOC (non-test) | ~9,100 (was ~7,200, +1,900) |
| Test LOC | ~3,200 (was ~3,000) |
| Unsafe Code | 0 (forbidden) |
| Panics in Prod | 0 |
| TUI Tests | 81 (unchanged) |
| Total Tests | 1,143 (was 756 in Phase 6, +387 gap-closure tests) |

### Top 5 Files by Size
1. openai_compatible.rs — 3,681 tokens (3.6%)
2. skills/parser.rs — 2,488 tokens (2.4%)
3. tui/app.rs — 2,456 tokens (2.4%)
4. query_engine.rs — 2,331 tokens (2.3%)
5. tasks/runner.rs — 2,140 tokens (2.1%)

### Crate Breakdown (LOC estimate) — Gap Closure Update
| Crate | LOC | Phase | Primary Role |
|-------|-----|-------|--------------|
| oxicode-common | 200 | 1 | Shared types |
| oxicode-config | 150 | 1 | Config loading |
| **oxicode-api** | **900** | **GC** | **Provider traits + multipart + policy limits** |
| **oxicode-core** | **1,200** | **GC** | **Query execution + AutoDream + VCR + PerfMetrics** |
| oxicode-tools | 520 | 3 | Tool system (42 tools) |
| oxicode-permissions | 300 | 1 | Access control |
| oxicode-state | 150 | 1 | State management |
| oxicode-session | 180 | 4 | Persistence |
| **oxicode-hooks** | **1,200** | **GC** | **Event hooks + DNS pinning (TOCTOU protection)** |
| oxicode-context | 450 | 4 | Context defense |
| oxicode-agents | 280 | 4 | Multi-agent |
| oxicode-skills | 350 | 4 | Skills system |
| oxicode-tasks | 380 | 4 | Background tasks |
| oxicode-plugins | 2,100 | 5 | Plugin marketplace |
| oxicode-voice | 400 | 7 | Voice input + Whisper |
| oxicode-remote | 420 | 7 | WebSocket bridge |
| oxicode-telemetry | 380 | 7 | OTLP collection |
| oxicode-github | 380 | 7 | GitHub App |
| **oxicode-mcp** | **1,050** | **GC** | **MCP client/server + 9 bridge debug modules** |
| **oxicode-tui** | **850** | **GC** | **Terminal UI + new stubs (TUI permission/config/notification)** |
| **oxicode-cli** | **650** | **GC** | **Commands + 5 gap-closure commands** |
| **Total** | **~12,000** | **GC** | **20 crates, 1,143 tests, 0 unsafe code** |

**Gap Closure Summary:** +24 files, +1,900 LOC, +387 tests across 6 crates (core, api, hooks, mcp, cli, tui)

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

## Next Steps (Phase 7+)

1. ✅ **Phase 7 COMPLETE** — Voice input, bridge mode, telemetry/OTLP, GitHub integration
2. **Phase 8:** UX Polish — Vim mode, keybindings, onboarding (COMPLETE ✓)
3. **Phase 9+:** Advanced features — OAuth, GitHub SSO, advanced team features

---

## References for Developers

- **Architecture:** See `system-architecture.md`
- **Code Standards:** See `code-standards.md`
- **Project Overview:** See `project-overview-pdr.md`
- **Exploration Report:** `plans/reports/Explore-260402-oxicode-phase4-interfaces.md`
- **Phase 4 Review:** `plans/reports/code-reviewer-260402-0830-phase4-quality.md`

