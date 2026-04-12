# QueryEngine: Multi-Turn Loop & Context Management

> **Design Document** | Core request processing, tool execution, and context budgeting  
> **Status**: Complete | **Related**: [01-overview](./01-overview-pdr.md), [03-tool-system](./03-tool-system.md), [04-permission-system](./04-permission-system.md)

---

## 1. QueryEngine Struct

The `QueryEngine` is the heart of OxiCode — it manages multi-turn conversations, routes requests to the LLM provider, executes tools with permission checks, and enforces context budgets.

(source: `crates/oxicode-core/src/query_engine.rs`)

### Structure Definition

```rust
pub struct QueryEngine {
    // LLM provider (Anthropic, OpenAI-compatible, Bedrock, Vertex)
    provider: Arc<dyn LlmProvider>,
    
    // Centralized app state (messages, settings, rate limits, etc.)
    pub(crate) state_store: Arc<StateStore>,
    
    // Registry of all 49+ built-in tools
    pub(crate) tool_registry: Arc<ToolRegistry>,
    
    // 6-layer permission pipeline (safe-allowlist → hard-deny → modes → AI → rules → ask)
    pub(crate) permission_pipeline: Arc<PermissionPipeline>,
    
    // Context passed to tools (working dir, task manager, MCP, skills, teams, bash processes)
    pub(crate) tool_context: ToolContext,
    
    // Active model name (can be switched at runtime via set_model())
    model: StdMutex<String>,
    
    // Max output tokens for LLM requests (u32)
    max_tokens: u32,
    
    // Base system prompt (assembled from CLAUDE.md, active skills, memory)
    system_prompt: String,
    
    // Context budget manager — tracks token usage, applies compaction when needed
    budget_manager: Mutex<BudgetManager>,
}
```

### Construction

```rust
pub fn new(
    provider: Arc<dyn LlmProvider>,
    state_store: Arc<StateStore>,
    tool_registry: Arc<ToolRegistry>,
    permission_pipeline: Arc<PermissionPipeline>,
    tool_context: ToolContext,
    model: String,
    max_tokens: u32,
    system_prompt: String,
) -> Self
```

### Thread Safety & Async Model

- **Sync Arc**: Provider, state, tools, permissions, tool context are shared via `Arc<T>` for cheap cloning
- **Model switching**: `StdMutex<String>` allows runtime model swaps (e.g., user selects "claude-3-opus")
- **Budget manager**: Wrapped in `tokio::sync::Mutex` because context defense needs exclusive access (`&mut self`)
- **Cancellation**: `execute_turn_with_cancel()` accepts an `Arc<AtomicBool>` flag polled at 50ms intervals to interrupt streaming or tool execution

---

## 2. Multi-Turn Loop State Machine

The core loop runs in `execute_turn_with_cancel()` — it sends messages to the LLM, collects the response, executes tools, and loops until the LLM signals stop (EndTurn) or we hit the max turn limit (50).

### State Diagram

```
START
  ↓
┌─────────────────────────────────────────────────────┐
│ PREPARE: Check cancel flag, apply budget defense    │
│ (L1=80%, L2=85%, L3=90%, Critical=98%)             │
│ return: Ok(compacted_msgs) | Err                    │
└────────────────────────┬────────────────────────────┘
                         ↓
┌─────────────────────────────────────────────────────┐
│ STREAM: stream_one_turn(conversation, event_tx)     │
│ collect text/thinking/tool_use/usage/stop_reason   │
│ emit TurnEvent::TextDelta, TurnEvent::ThinkingDelta │
│ return: Message { content: [Text|ToolUse|...] }    │
└────────────────────────┬────────────────────────────┘
                         ↓
┌─────────────────────────────────────────────────────┐
│ COLLECT: Extract all ToolUse blocks from message    │
│ if none OR stop_reason == EndTurn → RETURN result   │
│ else: continue to DISPATCH                          │
└────────────────────────┬────────────────────────────┘
                         ↓
┌─────────────────────────────────────────────────────┐
│ DISPATCH: For each tool use:                        │
│   - Check permission pipeline                       │
│   - Execute (with cancel flag interrupt support)    │
│   - Collect result (ContentBlock::ToolResult)       │
│   - Emit TurnEvent::ToolResult                      │
│ build Message { role: User, content: [results] }    │
│ push to state_store & conversation                  │
└────────────────────────┬────────────────────────────┘
                         ↓
┌─────────────────────────────────────────────────────┐
│ CHECK: if stop_reason != ToolUse → RETURN message   │
│ if turn_count > MAX_TOOL_TURNS (50) → ERROR         │
│ else: LOOP back to PREPARE                          │
└────────────────────────┬────────────────────────────┘
                         ↓
                    LOOP/STOP
```

### Pseudocode (31 Steps)

```
Function execute_turn_with_cancel(conversation, event_tx, cancel_flag):
  turn_count ← 0
  
  Loop:
    turn_count ← turn_count + 1                              // step 1
    
    if turn_count > MAX_TOOL_TURNS (50)                      // step 2
      log_warn("Max tool turns reached")
      break
    
    // PREPARE PHASE
    if cancel_flag present AND cancel_flag.load() == true   // step 3
      cancel_flag.store(false)
      state_store.set_streaming(false)
      emit(TurnEnd)
      return Err("Interrupted by user")
    
    // BUDGET DEFENSE (applies L1/L2/L3/L4/L5 as needed)     // step 4
    mut mgr ← lock(budget_manager)
    defended_msgs ← mgr.apply_defense_with_dir(              // step 5
        conversation.api_messages(),
        provider_ref(),
        model(),
        tool_context.working_dir
    )
    
    if defended_msgs.len() < conversation.len()              // step 6
      log_info("context compacted")
      conversation.replace_messages(defended_msgs)
    
    // STREAM PHASE
    mut assistant_msg ← stream_one_turn(                     // step 7
        conversation,
        event_tx,
        cancel_flag
    )
    
    stop_reason ← assistant_msg.stop_reason                  // step 8
    
    // COLLECT PHASE
    mut tool_uses ← []                                       // step 9
    for content_block in assistant_msg.content:
      if ContentBlock::ToolUse { id, name, input }:          // step 10
        tool_uses.push((id, name, input))
    
    if tool_uses.empty() OR stop_reason == EndTurn:          // step 11
      return Ok(assistant_msg)
    
    // DISPATCH PHASE
    mut tool_results ← []                                    // step 12
    mut interrupted ← false
    
    for (id, name, input) in tool_uses:                      // step 13
      
      if cancel_flag present AND cancel_flag.load():         // step 14
        cancel_flag.store(false)
        interrupted ← true
        break
      
      if cancel_flag present:                                // step 15
        // Race tool execution against cancel flag
        select:
          result ← execute_tool(id, name, input, event_tx)
          OR after 100ms if cancel_flag.load()
            interrupted ← true
            break
      else:
        result ← await execute_tool(id, name, input, event_tx) // step 16
      
      tool_results.push(result)                              // step 17
    
    if interrupted:                                          // step 18
      emit(ToolResult { is_error: true, content: "Interrupted" })
      state_store.set_streaming(false)
      emit(TurnEnd)
      return Err("Interrupted by user")
    
    // BUILD TOOL RESULT MESSAGE
    result_msg ← Message {                                   // step 19
      role: User,
      content: tool_results,
      ...
    }
    
    state_store.push_message(result_msg)                     // step 20
    conversation.push(result_msg)                            // step 21
    
    // CHECK PHASE
    if stop_reason != ToolUse:                               // step 22
      return Ok(assistant_msg)
    
    // Loop back to PREPARE
  
  return Err("Max tool turns exceeded")                       // step 23
```

### Cancellation Model

The `cancel_flag: Arc<AtomicBool>` provides graceful interruption:

1. **Before each turn**: Flag checked before streaming (step 3)
2. **During streaming**: Polled at 50ms intervals via `tokio::select!` (prevents 1+ second hangs)
3. **During tool execution**: Polled every 100ms; if set, the tool future is dropped (kills child processes like bash)
4. **30s timeout**: Permission dialogs timeout after 30s if user doesn't respond

---

## 3. Streaming Protocol

`stream_one_turn()` opens a streaming connection to the LLM and incrementally collects the response into an assistant Message.

(source: `crates/oxicode-core/src/query_engine.rs`, `crates/oxicode-api/src/stream_event.rs`)

### StreamEvent Enum

Events emitted by the provider's streaming connection:

```rust
pub enum StreamEvent {
    // Text content increments
    TextDelta { text: String },
    
    // Extended thinking (Claude 3.5+)
    ThinkingDelta { thinking: String },
    
    // Tool use block starts (id, name acquired)
    ToolUseStart { id: String, name: String },
    
    // Partial JSON input for tool being accumulated
    ToolInputDelta { partial_json: String },
    
    // Content block finished (index used to correlate blocks)
    ContentBlockStop { index: u32 },
    
    // Token usage update (input, output, cache metrics)
    UsageUpdate(Usage),
    
    // Message complete (end_turn | tool_use | max_tokens | stop_sequence)
    MessageStop { stop_reason: StopReason },
    
    // Rate limit detected — retry in progress
    RateLimited { info, attempt, max_retries, retry_in_secs },
    
    // Prompt cache invalidation detected (Anthropic-specific)
    CacheBreakDetected(CacheBreakEvent),
    
    // Non-rate-limit retry (502, connection error, etc.)
    Retrying { message, attempt, max_retries, retry_in_secs },
    
    // Stream error
    Error { message: String },
    
    // Keep-alive ping
    Ping,
}
```

### Text/Thinking Accumulation

Deltas are **streamed as chunks** and accumulated into ContentBlocks:

```
TextDelta { text: "Hello" }  ─→ current_text = "Hello"
TextDelta { text: " world" } ─→ current_text = "Hello world"
ContentBlockStop             ─→ push ContentBlock::Text { text: "Hello world" }
                                current_text = ""
```

Thinking follows the same pattern:

```
ThinkingDelta { thinking: "Let me..." }
ThinkingDelta { thinking: " analyze" }
ContentBlockStop
→ ContentBlock::Thinking { thinking: "Let me... analyze" }
```

### Tool Use Streaming

When a tool_use block streams:

```
ToolUseStart { id: "tool_xxx", name: "bash" }
  → current_tool_id = "tool_xxx"
  → current_tool_name = "bash"

ToolInputDelta { partial_json: "{\"command" }
  → current_tool_input_json = "{\"command"

ToolInputDelta { partial_json: "\":\"ls\"}" }
  → current_tool_input_json = "{\"command\":\"ls\"}"

ContentBlockStop
  → parse current_tool_input_json → serde_json::Value
  → push ContentBlock::ToolUse { id, name, input: Value }
  → current_tool_id = ""
```

### TurnEvent Emission to TUI

For each StreamEvent, a corresponding TurnEvent is emitted to the `event_tx` channel:

- `TextDelta` → `TurnEvent::TextDelta(text)`
- `ThinkingDelta` → `TurnEvent::ThinkingDelta(thinking)`
- `ToolUseStart` → `TurnEvent::ToolUseStart { id, name, input }`
- `ToolResult` (from tool dispatch) → `TurnEvent::ToolResult { tool_use_id, content, is_error }`
- `Retrying` / `RateLimited` → relayed directly
- `Error` → `TurnEvent::Error(message)`
- Message end → `TurnEvent::TurnEnd`

The TUI renders these real-time:
- Text appears as the user types
- Tool calls show input/output
- Permission dialogs block until user responds
- Retries/rate limits display in status

---

## 4. Tool Dispatch

When a tool use is detected in the assistant message, the engine executes it via the permission pipeline.

(source: `crates/oxicode-core/src/tool_dispatch.rs`)

### Execution Flow

```
1. extract_tool_use(id, name, input) from Message
            ↓
2. permission_pipeline.check(name, level, input)
            ↓
3. match decision:
      Allow     → run_tool(id, name, input)
      Deny      → return ToolResult { is_error: true, content: reason }
      Ask       → handle_permission_ask(id, name, input, prompt, event_tx)
            ↓
4. route execution:
      registry.get(name) exists?  → registry.execute(...)
      MCP tool?                   → mcp_manager.call_tool(...)
      not found?                  → error
            ↓
5. tool.execute(input, ctx) → ToolResult { content, is_error }
            ↓
6. wrap in ContentBlock::ToolResult { tool_use_id, content, is_error }
            ↓
7. emit TurnEvent::ToolResult
            ↓
8. add to tool_results vec
```

### Permission Ask Dialog Flow

When permission decision is `Ask`:

```
1. Create oneshot channel: (reply_tx, reply_rx)

2. Emit TurnEvent::PermissionAsk {
     tool_name,
     input_summary,  // truncated/formatted input for display
     prompt,         // reason (e.g. "This will write to /etc/passwd")
     reply_tx
   }

3. TUI receives event, shows dialog with 4 buttons:
   - Allow Once        → PermissionResponse::AllowOnce
   - Always Allow      → PermissionResponse::AlwaysAllow → add to pipeline
   - Deny              → PermissionResponse::Deny
   - Always Deny       → PermissionResponse::AlwaysDeny → add to pipeline

4. Engine waits on reply_rx with 30s timeout:
   
   AllowOnce:    → run_tool() immediately
   AlwaysAllow:  → add_session_allow(tool_name) → run_tool()
   Deny:         → return error ToolResult
   AlwaysDeny:   → add_session_deny(tool_name) → return error ToolResult
   Timeout:      → return ToolResult { is_error: true, content: "timed out" }
```

### Concurrent vs Sequential Execution

**Current: Sequential** — each tool runs to completion before the next starts. This ensures:
- Output order matches tool order
- Results can depend on prior tool side effects
- No race conditions on shared resources (files, state)

If parallelization is needed in future, each tool execution task would need:
- Unique `task_id` for bash process tracking
- Independent file locks
- Careful ordering of result collection

---

## 5. Conversation Management

The `Conversation` struct maintains the ordered message list for API requests.

(source: `crates/oxicode-core/src/conversation.rs`)

### Structure

```rust
pub struct Conversation {
    pub messages: Vec<Message>,  // chronological order
}
```

### Methods

| Method | Behavior |
|--------|----------|
| `push(msg)` | Append message (user or assistant) |
| `api_messages()` | Return all messages (never system-level) |
| `len()` | Message count |
| `is_empty()` | True if no messages |
| `replace_messages(vec)` | Atomically swap all messages (used after compaction) |

### System Prompt Injection

The system prompt is **not** stored in `Conversation` — it's managed by QueryEngine:

1. `QueryEngine::system_prompt` field holds base prompt
2. `build_request()` dynamically injects mode-specific text per turn (advisor_mode, sandbox_mode)
3. `MessageRequest` includes the final assembled prompt

(source: `crates/oxicode-core/src/system_prompt.rs`)

Assembly steps:
- Start with `BASE_SYSTEM_PROMPT`
- Append mode injection (if active skills include "advisor_mode" or "sandbox_mode")
- Append global CLAUDE.md content
- Append project CLAUDE.md
- Append project memory
- Append active skills list

---

## 6. Context Budget System

OxiCode enforces a 5-layer context defense to prevent context window overflow:

(source: `crates/oxicode-context/src/budget.rs` and all compaction strategy modules)

### BudgetManager Structure

```rust
pub struct BudgetManager {
    pub model_max_tokens: usize,     // e.g., 200,000 for Claude 3.5
    pub counter: TokenCounter,        // estimates tokens via marker counting
}
```

### Budget Status Thresholds

| Status | Ratio | Trigger | Defense |
|--------|-------|---------|---------|
| **Ok** | < 80% | None | No action |
| **L1** | 80–85% | NeedsL1Truncation | Truncate oldest messages |
| **L2** | 85–90% | NeedsL2Microcompact | L1 + snip tool results + microcompact |
| **L3** | 90–98% | NeedsL3AutoCompact | L1 + L2 + LLM-powered auto-summary |
| **Critical** | ≥ 98% | Critical | L4 (reactive) + L5 (collapse) |

### Defense Layers (Applied Sequentially)

#### L1: Truncation (80% budget)

(source: `crates/oxicode-context/src/truncation.rs`)

**Goal**: Keep only the most recent messages within 80% of max tokens.

- Discard oldest messages until token count < L1 threshold
- Preserves conversation continuity by removing from the front
- Fast O(n) operation

**Pseudocode**:
```
while count_tokens(messages) > l1_budget:
  messages.remove_first()
```

#### L1.5: Snip Compact (Optional)

(source: `crates/oxicode-context/src/snip_compact.rs`)

**Goal**: Trim verbose tool results (e.g., long file listings, grep output).

- Truncate tool results to first 200 + last 100 chars
- Replace middle with `[... N chars omitted ...]`
- Preserves structure for LLM understanding

#### L2: Microcompact (85% budget)

(source: `crates/oxicode-context/src/microcompact.rs`)

**Goal**: Compress thinking and tool results without LLM.

- Replace long tool results with `TOOL_RESULT_SUMMARY: { name, exit_code, char_count }`
- Remove thinking blocks (keep only output)
- Removes noise without losing semantics

#### L3: Auto-Compact (90% budget)

(source: `crates/oxicode-context/src/auto_compact.rs`)

**Goal**: LLM-powered summarization of conversation history.

- **Step 1**: Extract recent tool uses (last 5) via `post_compact_cleanup::extract_recent_tools()`
- **Step 2**: Send old messages to LLM: "Summarize this conversation for context"
- **Step 3**: Replace all old messages with single `summary_msg`
- **Step 4**: Restore recent tool contexts so LLM can reference them

**Pseudocode**:
```
recent_tools ← extract_recent_tools(messages, 5)  // keep last 5 tool uses
summary_msg ← AutoCompactor::compact(old_messages)
result = [summary_msg]
post_compact_restore(result, RestoreContext { recent_tools })  // re-inject tools
```

#### L4: Reactive Compact (Critical)

(source: `crates/oxicode-context/src/reactive_compact.rs`)

**Goal**: Urgently apply L1 + L2 + L3 under duress.

- **Faster**: Skips post-compact restore
- **Aggressive**: Chains truncation → microcompact → auto-compact
- If still critical after L4, proceed to L5

#### L5: Context Collapse (Emergency)

(source: `crates/oxicode-context/src/context_collapse.rs`)

**Goal**: Last resort — extract project structure and summarize.

- Scans working directory for codebase overview
- Generates high-level project structure
- Replace all conversation with single context-collapse message

---

## 7. Auxiliary Modules

### Token Counter

(source: `crates/oxicode-context/src/token_counter.rs`)

Estimates token counts without calling the API:

```rust
pub struct TokenCounter {
    // Cached estimates per Message, invalidated on mutation
}
pub fn count_messages(&mut self, msgs: &[Message]) -> usize
pub fn clear_cache(&mut self)
```

Uses heuristic: ~1.3 tokens per word (reasonable for Claude).

### Turn Event Emission

(source: `crates/oxicode-core/src/turn_event.rs`)

```rust
pub enum TurnEvent {
    TextDelta(String),
    ThinkingDelta(String),
    TurnStart,
    TurnEnd,
    ToolUseStart { id, name, input: Value },
    ToolResult { tool_use_id, content, is_error },
    PermissionAsk { tool_name, input_summary, prompt, reply_tx },
    Error(String),
    Retrying { message, attempt, max_retries, retry_in_secs },
    RateLimited { message, attempt, max_retries, retry_in_secs },
}

pub async fn emit(
    tx: Option<&tokio::sync::mpsc::Sender<TurnEvent>>,
    event: TurnEvent
) {
    if let Some(tx) = tx {
        let _ = tx.send(event).await;  // ignores closed receiver
    }
}
```

Used by TUI to render real-time progress. Single-prompt mode (`None` sender) has no overhead.

---

## 8. Edge Cases & Error Recovery

### Empty Content

If a tool returns `content: ""`:
- Wrapped in `ToolResult { is_error: false, content: "" }`
- LLM sees the tool ran successfully but produced no output
- LLM can decide to retry or continue

### Oversized Tool Result

If a tool result exceeds typical limits:
1. **Snip Compact** truncates to 200+100 chars
2. If still critical after L2, **Microcompact** replaces with summary
3. If still critical, **L3 Auto-Compact** runs

### Budget Critical During Tool Execution

**Scenario**: L3 compact triggers while a long-running bash command is executing.

**Resolution**:
- L3 compact applies between turns (before next API call)
- Long-running tools are never interrupted mid-flight by budget
- After tool completes, if budget critical, compaction applies before sending results to LLM

### Interrupt During Stream

**Scenario**: User presses Ctrl+C while LLM is streaming.

**Resolution**:
1. `cancel_flag.store(true)`
2. Streaming loop checks flag at 50ms intervals
3. On detection: finalize any pending text, set `stop_reason = EndTurn`, emit TurnEnd
4. Return early with partial message
5. No tool execution happens

### Interrupt During Tool Execution

**Scenario**: User presses Ctrl+C while bash is running.

**Resolution**:
1. `cancel_flag.store(true)`
2. Tool execution task polls flag every 100ms
3. On detection: drop the future (kills child process via `kill_on_drop(true)`)
4. Return `ToolResult { is_error: true, content: "Interrupted by user" }`
5. Resume normal turn loop

### Model Switch Mid-Conversation

**Scenario**: User runs `model set claude-3-5-sonnet` mid-turn.

**Resolution**:
1. `set_model(new_model)` updates `model: StdMutex<String>`
2. Next turn's `build_request()` uses the new model
3. Token counting may differ (L1/L2/L3 thresholds recalculate)
4. LLM might have different context window → budget re-evaluated

### Rate Limit 429

**Scenario**: Provider returns 429 (too many requests).

**Resolution**:
1. `RateLimited` event emitted with retry info
2. `oxicode-api` automatically retries (exponential backoff)
3. Engine sees events and emits `TurnEvent::RateLimited` to TUI
4. No action needed from engine — provider handles retry

### Connection Error (502, timeout)

**Scenario**: Network hiccup or provider unavailable.

**Resolution**:
1. `Retrying` event emitted
2. Provider retries per its policy
3. If max retries exceeded, `Error` event + engine returns `Err`
4. Conversation state preserved; user can retry

---

## 9. Constants & Configuration

(source: `crates/oxicode-core/src/query_engine.rs`)

```rust
const MAX_TOOL_TURNS: usize = 50;           // absolute loop limit
const DEFAULT_MODEL_MAX_TOKENS: usize = 200_000;  // Claude 3.5 typical
```

(source: `crates/oxicode-context/src/budget.rs`)

```rust
const L1_THRESHOLD: f64 = 0.80;             // 80% full
const L2_THRESHOLD: f64 = 0.85;             // 85% full
const L3_THRESHOLD: f64 = 0.90;             // 90% full
const CRITICAL_THRESHOLD: f64 = 0.98;       // 98% full
```

---

## 10. Integration Points

→ See [03-tool-system.md](./03-tool-system.md) for tool execution details  
→ See [04-permission-system.md](./04-permission-system.md) for permission pipeline  
→ See [05-state-management.md](./05-state-management.md) for StateStore integration  
→ See [06-api-providers.md](./06-api-providers.md) for LlmProvider implementations

---

**Document Info**  
Lines: ~750 | Last updated: 2026-04-12 | Version: 1.0
