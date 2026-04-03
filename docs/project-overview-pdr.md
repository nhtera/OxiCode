# OxiCode — Project Overview & PDR

**Version:** 0.3.0 | **Last Updated:** 2026-04-03 | **Status:** Phase 1-8 Complete (UX Polish done)

## Project Vision

OxiCode is a multi-agent, Rust-powered CLI assistant for software engineering. It combines:
- **LLM-driven code reasoning** (Claude API)
- **Multi-provider support** (OpenAI, Anthropic, local via MCP)
- **Context defense** (5-layer token budget system)
- **Extensible tooling** (33+ built-in tools, custom tool trait)
- **Team collaboration** (multi-agent coordination, skill plugins, background tasks)
- **TUI interface** (ratatui, real-time streaming, responsive design)

---

## Phase Overview

### Phase 1-3: Foundation (Complete)
- Core LLM integration (stream, tools, thinking)
- Multi-provider support (OpenAI, Anthropic, compatible APIs)
- TUI frontend with streaming display
- Permission-based tool access control
- Session persistence and undo/redo

### Phase 2: API Enhancement (Complete ✓)
**Enhanced:** oxicode-api crate with multi-provider support
- **Prompt Caching** — Request-level flag for Anthropic prompt caching
- **Extended Thinking** — ThinkingConfig with token budget (min 1024)
- **AWS Bedrock Provider** — SigV4 auth, event-stream parsing, env var auto-detection
- **Google Vertex AI Provider** — OAuth2 bearer auth, standard SSE streaming
- **Provider Router Enhancement** — Auto-detect Bedrock/Vertex from env vars, model prefix routing

### Phase 4: Multi-Agent & Skills (Complete ✓)
**Added:** 4 new crates, 31 new files, ~2800 LOC
- **oxicode-context** — 5-layer context defense + token counting
- **oxicode-agents** — Subagent spawning, coordinator mode, team management
- **oxicode-skills** — Skill discovery, markdown parsing, activation
- **oxicode-tasks** — Background task management, async process execution
- **Integration** — Context defense hooks in QueryEngine, TUI widgets for agents/tasks/notifications

### Phase 3: Gap Closure (Complete ✓)
**Added:** 11 new tools to oxicode-tools crate for OpenClaude feature parity
- **TodoWriteTool** — Write todo lists and task management
- **TeamCreateTool, TeamDeleteTool** — Team management with TeamManager integration
- **LspTool** — Language Server Protocol integration for IDE features
- **PowerShellTool** — PowerShell script execution (Windows/cross-platform)
- **ReplTool** — Interactive REPL environment
- **McpAuthTool** — MCP authentication and credential management
- **SuggestBackgroundPrTool** — Background PR suggestion and generation
- **SyntheticOutputTool** — Generate synthetic test output and data
- **VerifyPlanExecutionTool** — Plan execution verification and validation
- **WorkflowTool** — Workflow automation and orchestration
- **Tool Count:** Increased from 31 to 42 (of 44 in OpenClaude)

### Phase 6: Server Mode (Complete ✓)
**Added:** JSON-RPC 2.0 server protocol, IDE bridge, session management
- **Server Protocol** — JSON-RPC over stdin/stdout, permission bridge flow
- **Session Management** — Multi-session support, resumable conversations
- **Streaming Notifications** — stream.text, tool.start, tool.result, permission.ask
- **Cancellation** — Token-based request cancellation via CancellationToken

### Phase 7: Task System + Feature Flags (Complete ✓)
**Added:** Background task management, async execution, JSONL output streaming
- **TaskManager** — In-process registry, status tracking
- **TaskRunner** — Async process spawning, output redirection
- **OutputReader** — Incremental JSONL streaming with polling
- **Feature Flags** — Beta feature toggles, configuration-driven activation

### Phase 8: UX Polish (Complete ✓)
**Added:** 3 new modules, full vim mode, configurable keybindings, onboarding wizard
- **Vim Mode** — Full state machine (Normal/Insert/Visual/Command), hjkl motions, operators (dd, yy, dw, etc)
- **Keybindings** — TOML-based customization, chord sequences, defaults registry
- **Onboarding Wizard** — First-run setup (API key, model, permissions, theme), masked input, config generation
- **Output Styles** — markdown, plain, minimal, verbose rendering modes
- **Enhanced Input** — Multi-line editing, command history, history search, readline shortcuts, word-level movement
- **Test Coverage** — 59 passing tests (50 TUI + 9 config), zero clippy warnings

### Phase 9+: Forthcoming
- **Phase 9 (Enterprise):** OAuth, GitHub SSO, telemetry, audit logging
- **Phase 10 (Extras):** Voice input, advanced bridging, community plugins

---

## Crate Architecture (16 crates total)

### Foundational Layer
| Crate | Purpose | Key Type | Lines |
|---|---|---|---|
| **oxicode-common** | Shared types, errors, models | `OxiError`, `Message`, `Usage` | 200 |
| **oxicode-config** | Configuration loading, env vars | `Config`, `Provider` | 150 |
| **oxicode-api** | LLM provider traits, message types | `LlmProvider`, `StreamEvent` | 600 |

### Core Execution Layer
| Crate | Purpose | Key Type | Lines |
|---|---|---|---|
| **oxicode-core** | Query engine, system prompt assembly | `QueryEngine` | 400 |
| **oxicode-tools** | Tool registry, execution, schema | `Tool`, `ToolRegistry` | 350 |
| **oxicode-permissions** | Permission pipeline, access control | `PermissionPipeline` | 300 |

### State & Persistence Layer
| Crate | Purpose | Key Type | Lines |
|---|---|---|---|
| **oxicode-state** | App state store, watch channels | `StateStore`, `AppState` | 150 |
| **oxicode-session** | Session persistence (JSONL) | `SessionManager` | 180 |
| **oxicode-hooks** | Pre-commit, post-exec callbacks | `HookManager` | 120 |

### Phase 4: Context Defense & Multi-Agent Layer
| Crate | Purpose | Key Type | Lines |
|---|---|---|---|
| **oxicode-context** | Token counting, 5-layer defense | `BudgetManager`, `TokenCounter` | 450 |
| **oxicode-agents** | Subagent spawning, coordination | `AgentHandle`, `CoordinatorState` | 280 |
| **oxicode-skills** | Skill discovery, parsing, activation | `SkillDiscovery`, `SkillExecutor` | 350 |
| **oxicode-tasks** | Background task runner, output streaming | `TaskManager`, `TaskRunner` | 380 |

### Integration & UI Layer
| Crate | Purpose | Key Type | Lines |
|---|---|---|---|
| **oxicode-mcp** | MCP server wrapper for external tools | `McpServer` | 200 |
| **oxicode-tui** | Terminal UI, event loop, rendering | `App`, `Renderer` | 600 |
| **oxicode-cli** | Slash commands, REPL, CLI parsing | `CommandRegistry`, `SlashCommand` | 280 |

---

## Key Features

### 1. Context Defense (5 Layers)
**Problem:** LLM conversations grow unbounded; expensive tokens, slow response.

**Solution:** Graduated token management:
1. **L1: Truncation** — Remove oldest middle messages until under budget
2. **L2: Microcompact** — In-place compression of tool results + thinking blocks
3. **L3: Auto-Compact** — LLM-assisted conversation summarization (triggered at 70% budget)
4. **L4: Reactive** — Mid-turn emergency compaction during streaming (triggered at 95% budget)
5. **L5: Collapse** — Last-resort context reset from working directory state (triggered at 100% budget)

**Integration:** `BudgetManager` orchestrates layers; `QueryEngine` hooks context defense into stream loop.

### 2. Multi-Provider Support (Phase 2 Enhanced)
- **Anthropic Claude** — Primary provider, prompt caching, extended thinking
- **OpenAI Compatible** — OpenAI, Azure, DeepSeek, OpenRouter, Ollama
- **AWS Bedrock** — SigV4-signed requests, event-stream parsing
- **Google Vertex AI** — OAuth2 bearer auth, standard SSE streaming
- **MCP (Model Context Protocol)** — External tools via stdin/stdout bridge
- **Provider Router** — Auto-detect providers from env vars, model prefix routing

### 3. Permission Pipeline (6 Layers)
1. Safe allowlist (read-only always allowed)
2. User permission mode (Default/Bypass/ApprovalOnly)
3. Command security (hard deny dangerous patterns)
4. Pattern detection (suspicious file ops)
5. Rule matching (user-configured allow/deny)
6. Default ask behavior

### 4. Tool System (42 Built-in + Custom)
**Built-in (42 total):**
- **Core I/O (6):** file_read, file_write, file_edit, glob_tool, grep_tool, bash
- **User Interaction (3):** ask_user, send_message, config_tool
- **Specialized (3):** notebook_edit, tool_search, skill
- **Task Management (7):** task_create, task_get, task_list, task_update, task_stop, task_output
- **Web Tools (2):** web_fetch, web_search
- **MCP Integration (2):** list_mcp_resources, read_mcp_resource
- **Phase 5 Workflow (7):** plan_mode (enter/exit), worktree (enter/exit), brief, structured_output, cron (create/delete/list), sleep, remote_trigger
- **Phase 3 Gap Closure (11):** todo_write, team_create, team_delete, lsp_tool, powershell, repl_tool, mcp_auth, suggest_background_pr, synthetic_output, verify_plan_execution, workflow_tool

**Custom:** Implement `Tool` trait, register via `ToolRegistry`.

**Phases:** Phase 1 (MCP resources + skill), Phase 5 (workflow + dev tools), Phase 3 (gap closure: 11 tools for OpenClaude parity)

### 5. Skill System
**Skills** are markdown files (`SKILL.md`) with YAML frontmatter that inject prompt snippets when activated.

- **Discovery:** Scan `~/.oxicode/skills/` + `./.oxicode/skills/`
- **Parsing:** Extract YAML metadata, prompt text
- **Activation:** Inject when conditions met (file type, user intent)
- **Execution:** Handled by `SkillExecutor`

### 6. Multi-Agent Coordination
- **Spawner:** Launch subagents with config (model, tools, max_tokens)
- **Coordinator:** One agent manages other agents (delegation, synthesis)
- **Team:** Group agents with shared state, messaging
- **Communication:** Inter-agent messages via `MessageBus`, JSON-based

### 7. Background Tasks
- **TaskManager:** In-process registry of long-running tasks
- **TaskRunner:** Async process spawning, I/O streaming to disk
- **OutputReader:** Incremental JSONL log reader
- **Notifications:** De-duplicating, non-blocking task status updates

---

## Product Development Requirements (PDRs)

### Functional Requirements
1. **Chat Interface** — Streaming LLM responses with tool results, user input loop
2. **Tool Execution** — Safe execution with permission checks, error handling
3. **Context Management** — Token-aware message history, graduated defense
4. **Multi-Provider** — Seamless switching between OpenAI, Anthropic, local
5. **Session Persistence** — Save/load conversations, undo/redo
6. **Multi-Agent** — Spawn, coordinate, communicate with subagents
7. **Skill System** — Load, activate, inject skill prompts dynamically
8. **Background Tasks** — Run and monitor async processes, stream output

### Non-Functional Requirements
- **Performance:** <2s LLM response time, <100ms UI frame time
- **Token Efficiency:** Context defense keeps conversation tokens <80% of budget
- **Security:** No shell injection, permission-based access, secret redaction
- **Reliability:** Graceful error handling, process recovery, no panics in prod
- **Extensibility:** Custom tools, skills, providers via trait implementation
- **Observability:** Structured logging (tracing), debug output, metrics

### Acceptance Criteria
- All tests pass (unit + integration)
- No `unsafe` code in production (clippy-approved)
- Context defense achieves <100ms compaction latency
- Agent spawning works end-to-end (config serialization, stdin/stdout)
- Skill discovery handles circular symlinks, temp cleanup
- Permission pipeline rejects all OWASP-defined dangerous patterns

---

## File Structure

```
oxicode/
├── crates/                          # 16 Rust workspaces
│   ├── oxicode-common/              # Shared types
│   ├── oxicode-config/              # Config loading
│   ├── oxicode-api/                 # LLM provider traits
│   ├── oxicode-core/                # Query engine, system prompt
│   ├── oxicode-tools/               # Tool registry, schema
│   ├── oxicode-permissions/         # Permission pipeline
│   ├── oxicode-state/               # App state, watch channels
│   ├── oxicode-session/             # Session persistence
│   ├── oxicode-hooks/               # Hook system
│   ├── oxicode-context/             # Context defense (Phase 4)
│   ├── oxicode-agents/              # Multi-agent (Phase 4)
│   ├── oxicode-skills/              # Skills system (Phase 4)
│   ├── oxicode-tasks/               # Background tasks (Phase 4)
│   ├── oxicode-mcp/                 # MCP server wrapper
│   ├── oxicode-tui/                 # TUI frontend
│   └── oxicode-cli/                 # Slash commands
├── docs/                            # Documentation (THIS DIR)
├── plans/                           # Implementation phases
├── tests/                           # Integration tests
├── Cargo.toml                       # Workspace manifest
└── rust-toolchain.toml              # Rust 1.80+
```

---

## Development Status

| Phase | Status | Deliverables |
|-------|--------|--------------|
| 1-3 | ✓ Complete | Core LLM, providers, tools, permissions, TUI |
| **2 (API)** | **✓ Complete** | **Prompt caching, extended thinking, Bedrock, Vertex** |
| **4** | **✓ Complete** | **Context defense, multi-agent, skills, tasks** |
| 5 | Planned | User commands, team UI panels, skill marketplace |

**Code Quality:** Phase 4 review found 4 critical security issues, 7 high-priority wiring/robustness gaps. **All 11 issues fixed and verified** (326 tests pass, clippy clean, cargo check passes).

**Test Coverage:** Unit tests for all modules, edge cases (empty input, zero budget, missing files), integration tests for provider routing, security penetration tests.

---

## How to Contribute

1. **Read** `./docs/codebase-summary.md` for architecture overview
2. **Check** `./docs/code-standards.md` for style + patterns
3. **Review** `./docs/system-architecture.md` for integration points
4. **Write** code following YAGNI/KISS/DRY
5. **Test** via `cargo test`, handle edge cases
6. **Document** APIs with doc comments, update docs/ as needed

---

## References

- **Repomix Summary:** 101,835 tokens, 121 files, 450K chars
- **Top 3 Files:** openai_compatible.rs (3.6%), skills parser (2.4%), app.rs (2.4%)
- **Production Ready:** All critical security and wiring issues fixed (2026-04-03)
- **Next:** Phase 5 user-facing commands, team UI, marketplace
