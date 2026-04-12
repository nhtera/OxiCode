# Tool System: Registry & Implementation Reference

> **Design Document** | Complete tool subsystem, 49+ built-in tools, MCP integration  
> **Status**: Complete | **Related**: [02-query-engine.md](./02-query-engine.md), [04-permission-system.md](./04-permission-system.md)

---

## 1. Tool Trait

The core abstraction for all tools in OxiCode.

(source: `crates/oxicode-tools/src/tool_trait.rs`)

### Trait Definition

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool identifier (e.g., "bash", "file_read", "web_search")
    fn name(&self) -> &str;
    
    /// Human-readable description for the LLM
    fn description(&self) -> &str;
    
    /// JSON Schema for input parameters (for LLM)
    fn schema(&self) -> ToolSchema;
    
    /// Permission level required to execute
    fn permission_level(&self) -> PermissionLevel;
    
    /// Execute the tool with JSON input and shared context
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> OxiResult<ToolResult>;
}
```

### Associated Types

#### ToolSchema

```rust
pub struct ToolSchema {
    pub name: String,                          // name
    pub description: String,                   // description
    pub input_schema: serde_json::Value,       // JSON Schema object
}
```

Example (bash):
```json
{
  "name": "bash",
  "description": "Execute shell commands",
  "input_schema": {
    "type": "object",
    "properties": {
      "command": { "type": "string" },
      "timeout": { "type": "integer", "default": 300 }
    },
    "required": ["command"]
  }
}
```

#### ToolResult

```rust
pub struct ToolResult {
    pub content: String,     // output (text, JSON, etc.)
    pub is_error: bool,      // whether tool encountered error
}

impl ToolResult {
    pub fn success(content: impl Into<String>) -> Self { ... }
    pub fn error(content: impl Into<String>) -> Self { ... }
}
```

#### ToolContext

```rust
pub struct ToolContext {
    pub working_dir: PathBuf,                    // cwd for file/command execution
    pub file_state: Arc<FileStateTracker>,       // mtime tracking (detect stale edits)
    pub task_manager: Arc<Mutex<TaskManager>>,   // background task management
    pub task_abort_handles: Arc<Mutex<HashMap<String, AbortHandle>>>,
    pub mcp_manager: Arc<McpServerManager>,      // MCP tool integration
    pub skill_executor: Option<Arc<SkillExecutor>>,  // skill invocation
    pub team_manager: Arc<Mutex<TeamManager>>,   // agent team management
    pub bash_processes: BashProcessMap,          // track running bash procs
}
```

#### PermissionLevel

```rust
pub enum PermissionLevel {
    ReadOnly,       // safe (auto-allow) — file_read, glob, grep, web_fetch
    FileWrite,      // file modifications — file_write, file_edit, notebook_edit
    ShellExec,      // command execution — bash, bash_background, powershell, cron
    System,         // system operations — team_create, mcp_auth, task_create
}
```

---

## 2. ToolRegistry

Central registration and execution dispatcher.

(source: `crates/oxicode-tools/src/registry.rs`)

### Structure

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,  // name → Box<Tool>
}
```

### Core Methods

| Method | Signature | Purpose |
|--------|-----------|---------|
| `register` | `(&mut self, tool: Box<dyn Tool>)` | Register a new tool |
| `get` | `(&self, name: &str) -> Option<&dyn Tool>` | Look up by name |
| `list` | `(&self) -> Vec<ToolSchema>` | All schemas |
| `names` | `(&self) -> Vec<String>` | All tool names |
| `schemas_json` | `(&self) -> Vec<Value>` | Schemas as JSON array |
| `execute` | `async (&self, name, input, ctx) -> OxiResult<ToolResult>` | Run tool by name |
| `len` | `(&self) -> usize` | Tool count |
| `is_empty` | `(&self) -> bool` | Check if empty |
| `retain` | `(&mut self, f: Fn(&str) -> bool)` | Filter tools (agent whitelists) |

### MCP Tool Integration

Built-in tools use `ToolRegistry::execute()`.  
MCP tools use `ToolContext::mcp_manager::call_tool()`.

(See **Tool Dispatch Flow** in section 8)

---

## 3. All 49+ Built-In Tools by Category

Listed as found in `crates/oxicode-tools/src/`:

### File Operations (ReadOnly / FileWrite)

| Tool | File | Input | Permission | Read-only? | Max Result |
|------|------|-------|-----------|-----------|-----------|
| **read** | `file_read.rs` | `file_path: String` | ReadOnly | ✅ | 500 KB |
| **write** | `file_write.rs` | `file_path: String, content: String` | FileWrite | ❌ | N/A |
| **edit** | `file_edit.rs` | `file_path, old_string, new_string` | FileWrite | ❌ | N/A |
| **glob** | `glob_tool.rs` | `pattern: String, path?: String` | ReadOnly | ✅ | 100 KB |
| **grep** | `grep_tool.rs` | `pattern, path?, glob?, output_mode?` | ReadOnly | ✅ | 200 KB |
| **notebook_edit** | `notebook_edit.rs` | `notebook_path, cell_id, new_source, edit_mode?` | FileWrite | ❌ | N/A |

### Shell / Process (ShellExec)

| Tool | File | Input | Permission | Special |
|------|------|-------|-----------|---------|
| **bash** | `bash.rs` | `command: String, timeout?: u32, cwd?: String` | ShellExec | ✅ Streaming, kill_on_drop |
| **bash_background** | `bash_background.rs` | `command: String, task_id: String` | ShellExec | Runs async |
| **kill_bash** | `kill_bash.rs` | `task_id: String` | ShellExec | Kills running process |
| **powershell** | `powershell.rs` | `command: String, timeout?: u32` | ShellExec | Windows-native |

### Code / Development (FileWrite / ReadOnly)

| Tool | File | Input | Permission | Purpose |
|------|------|-------|-----------|---------|
| **lsp** | `lsp_tool.rs` | `action: String, params: Object` | ReadOnly | Language server (goto-def, hover, etc.) |
| **notebook_edit** | `notebook_edit.rs` | (see above) | FileWrite | Jupyter notebook cell editing |

### Search & Fetch (ReadOnly)

| Tool | File | Input | Permission | Max Result |
|------|------|-------|-----------|-----------|
| **web_fetch** | `web_fetch.rs` | `url: String, prompt?: String` | ReadOnly | 100 KB |
| **web_search** | `web_search.rs` | `query: String, allowed_domains?: [String]` | ReadOnly | 50 KB |

### Task Management (System)

| Tool | File | Input | Permission | Purpose |
|------|------|-------|-----------|---------|
| **task_create** | `task_tools.rs` | `name, owner, description, files?, acceptance_criteria?` | System | Create background task |
| **task_list** | `task_tools.rs` | `filter?: String` | System | List all tasks |
| **task_get** | `task_tools.rs` | `task_id: String` | System | Fetch single task |
| **task_update** | `task_tools.rs` | `task_id, status?, owner?, summary?` | System | Update task state |

### Agent Orchestration (System)

| Tool | File | Input | Permission | Purpose |
|------|------|-------|-----------|---------|
| **agent** | `agent_tool.rs` | `type: String (researcher/planner/etc.), prompt: String, ...` | System | Spawn subagent |
| **send_message** | `send_message.rs` | `recipient: String, message: String, type?` | System | DM or broadcast |
| **team_create** | `team_tools.rs` | `name: String, members: [String]` | System | Create agent team |
| **team_delete** | `team_tools.rs` | `team_id: String` | System | Delete team |

### Configuration (System)

| Tool | File | Input | Permission | Purpose |
|------|------|-------|-----------|---------|
| **plan_mode** | `plan_mode.rs` | `action: "enter" \| "exit", plan_slug?: String` | System | Toggle plan mode |
| **ask_user** | `ask_user.rs` | `question: String, options?: [String]` | System | Prompt for input |
| **tool_search** | `tool_search.rs` | `query: String, limit?: u32` | ReadOnly | Find tools by name/desc |
| **config_tool** | `config_tool.rs` | `action: String, key?: String, value?: String` | System | Get/set config |

### Git & Versioning (ShellExec / FileWrite)

| Tool | File | Input | Permission | Purpose |
|------|------|-------|-----------|---------|
| **worktree** | `worktree.rs` | `action: "create" \| "list" \| "remove", ...` | ShellExec | Git worktree ops |
| **suggest_background_pr** | `suggest_background_pr.rs` | `title, body, branch?` | ShellExec | Auto-create PR in bg |

### Utility / Misc (ReadOnly / System)

| Tool | File | Input | Permission | Purpose |
|------|------|-------|-----------|---------|
| **brief** | `brief.rs` | `content: String` | ReadOnly | Summarize content |
| **sleep** | `sleep.rs` | `duration_secs: u32` | ReadOnly | Pause execution |
| **cron** | `cron.rs` | `schedule: String, command: String` | ShellExec | Schedule tasks |
| **remote_trigger** | `remote_trigger.rs` | `endpoint: String, method: String, body?: Object` | System | HTTP trigger |
| **repl_tool** | `repl_tool.rs` | `language: String, code: String` | ShellExec | Interactive REPL |
| **skill_tool** | `skill_tool.rs` | `skill_name: String, args?: Object` | System | Invoke discovered skills |
| **structured_output** | `structured_output.rs` | `format: "json" \| "yaml", content: String` | ReadOnly | Format output |
| **synthetic_output** | `synthetic_output.rs` | `type: String, data: Object` | ReadOnly | Generate fake output |
| **todo_write** | `todo_write.rs` | `file_path: String, todos: [String]` | FileWrite | Write checklist |
| **workflow_tool** | `workflow_tool.rs` | `action: String, ...` | System | Workflow orchestration |
| **verify_plan_execution** | `verify_plan_execution.rs` | `plan_id: String` | ReadOnly | Check plan status |

### MCP Integration (System)

| Tool | File | Input | Permission | Purpose |
|------|------|-------|-----------|---------|
| **mcp_auth** | `mcp_auth.rs` | `server_name: String, action: "start" \| "stop"` | System | MCP auth/lifecycle |
| **mcp_resource_tools** | `mcp_resource_tools.rs` | `resource_uri: String, action: "read" \| "write"` | System | MCP resource access |

### Summary Counts

- **ReadOnly**: 7 (file_read, glob, grep, web_fetch, web_search, brief, tool_search, etc.)
- **FileWrite**: 4 (file_write, file_edit, notebook_edit, todo_write)
- **ShellExec**: 7 (bash, bash_background, kill_bash, powershell, cron, repl, worktree)
- **System**: 18+ (tasks, agents, teams, config, skills, MCP, workflow, etc.)

**Total: ~45–49 tools** (exact count depends on feature flags)

---

## 4. Tool Execution Flow

ASCII diagram showing request → execution → result:

```
LLM Response Message
  │
  ├─ ContentBlock::ToolUse { id, name, input }
  │
  └─→ QueryEngine::execute_tool(id, name, input, event_tx)
      │
      ├─→ PERMISSION CHECK
      │   └─ permission_pipeline.check(name, level, input)
      │      │
      │      ├─ Allow      → proceed to RUN
      │      ├─ Deny       → return error ToolResult
      │      └─ Ask        → emit PermissionAsk, wait for user response
      │
      ├─→ ROUTE
      │   ├─ tool_registry.get(name) exists?
      │   │  └─ YES → built-in tool, proceed to RUN
      │   │
      │   ├─ MCP tool? (name contains "__")
      │   │  └─ YES → try_mcp_tool(name, input), proceed to RUN
      │   │
      │   └─ NOT FOUND → error ToolResult
      │
      ├─→ RUN
      │   └─ if built-in:
      │      tool_registry.execute(name, input, ctx)
      │        └─ tool.execute(input, ctx) → ToolResult
      │
      │      if MCP:
      │      mcp_manager.call_tool(server, tool, input)
      │        └─ process response → ToolResult
      │
      └─→ WRAP & EMIT
          └─ ContentBlock::ToolResult { tool_use_id, content, is_error }
          └─ TurnEvent::ToolResult { ... }
          └─ append to conversation
```

### Permission Pipeline Integration

(source: `crates/oxicode-core/src/tool_dispatch.rs`)

```
execute_tool(id, name, input, event_tx):
  
  1. Get tool permission level:
     - built-in: tool.permission_level()
     - MCP: System (conservatively)
  
  2. Check permission:
     decision = permission_pipeline.check(name, level, input)
  
  3. Route decision:
     if Allow:
       run_tool(id, name, input)
     
     if Deny(reason):
       return ToolResult::error(reason)
     
     if Ask(prompt):
       handle_permission_ask(id, name, input, prompt, event_tx)
       
       create oneshot: (reply_tx, reply_rx)
       emit TurnEvent::PermissionAsk { tool_name, input_summary, prompt, reply_tx }
       
       wait on reply_rx with 30s timeout:
         AllowOnce    → run_tool()
         AlwaysAllow  → add_session_allow(name) → run_tool()
         Deny         → return error
         AlwaysDeny   → add_session_deny(name) → return error
         Timeout      → return timeout error
```

### Run Tool

```
run_tool(id, name, input):
  
  if tool_registry.get(name):
    result = tool_registry.execute(name, input, ctx)
  else if try_mcp_tool(name, input):
    result = mcp_result
  else:
    result = Err("Tool not found")
  
  match result:
    Ok(ToolResult) → wrap in ContentBlock::ToolResult
    Err(e)        → ToolResult::error(e.to_string())
```

### Tool Input Summarization

(source: `crates/oxicode-core/src/tool_dispatch.rs`, `summarize_tool_input()`)

For permission dialogs, tool input is truncated to 80 chars:

```rust
match tool_name {
    "bash" => input["command"].as_str() → "cargo test --release"
    "file_read" | "file_write" | "file_edit" => input["file_path"] → "/src/main.rs"
    "grep" => format!("{} in {}", pattern, path) → "TODO in src/"
    "glob" => input["pattern"] → "**/*.rs"
    _ => serde_json::to_string(input) → "{\"key\":\"value\"}"
}

if len > 80:
  return format!("{}...", &s[..80])
else:
  return s
```

---

## 5. Adding a New Tool

Step-by-step guide to implement and register a new tool.

### Step 1: Create Tool File

Create `crates/oxicode-tools/src/my_awesome_tool.rs`:

```rust
use async_trait::async_trait;
use oxicode_common::{OxiError, OxiResult};
use crate::tool_trait::{Tool, ToolContext, ToolResult, ToolSchema, PermissionLevel};

pub struct MyAwesomeTool;

#[async_trait]
impl Tool for MyAwesomeTool {
    fn name(&self) -> &str {
        "my_awesome_tool"
    }
    
    fn description(&self) -> &str {
        "Does something awesome with inputs and context"
    }
    
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "my_awesome_tool".into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "param1": { "type": "string", "description": "First param" },
                    "param2": { "type": "integer", "description": "Second param" }
                },
                "required": ["param1"]
            }),
        }
    }
    
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly  // or FileWrite, ShellExec, System
    }
    
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> OxiResult<ToolResult> {
        let param1: String = input
            .get("param1")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OxiError::Tool {
                name: self.name().into(),
                message: "Missing param1".into(),
            })?
            .to_string();
        
        let param2: i32 = input
            .get("param2")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        
        // Use context as needed
        let _working_dir = &ctx.working_dir;
        let _file_state = &ctx.file_state;
        
        // Do the work
        let result = format!("Processed {} with {}", param1, param2);
        
        Ok(ToolResult::success(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_my_awesome_tool() {
        let tool = MyAwesomeTool;
        let input = serde_json::json!({
            "param1": "test",
            "param2": 42
        });
        let result = tool
            .execute(input, &ToolContext::default())
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("test"));
    }
}
```

### Step 2: Register in lib.rs

Edit `crates/oxicode-tools/src/lib.rs`:

```rust
mod my_awesome_tool;

pub fn build_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    
    // ... existing tools ...
    
    registry.register(Box::new(my_awesome_tool::MyAwesomeTool));
    
    registry
}
```

### Step 3: Set Permission Level

Choose based on what the tool does:

| Level | Suitable For | Example |
|-------|-------------|---------|
| `ReadOnly` | Info retrieval, no side effects | file_read, grep, web_search |
| `FileWrite` | Modifies files | file_write, file_edit, todo_write |
| `ShellExec` | Executes commands | bash, powershell, cron |
| `System` | System-level ops | task_create, agent, mcp_auth |

### Step 4: Test

```bash
cargo test --package oxicode-tools --lib my_awesome_tool
```

### Step 5: Document

Add entry to tool catalog (e.g., in `docs/`). Include:
- Name
- Description
- Input schema
- Permission level
- Use cases
- Examples

### Step 6: Add to Integration Tests

(Optional) Add end-to-end test in `crates/oxicode-cli/tests/`:

```rust
#[tokio::test]
async fn test_my_awesome_tool_e2e() {
    let mut engine = build_test_engine().await;
    let result = engine.execute_tool("my_awesome_tool", json!({
        "param1": "test"
    })).await;
    assert!(result.is_ok());
}
```

---

## 6. MCP Tool Integration

Built-in tools are registered with `ToolRegistry`.  
MCP tools (from external servers) are accessed dynamically.

(source: `crates/oxicode-core/src/tool_dispatch.rs`)

### Discovery & Invocation

```
try_mcp_tool(tool_name, input):
  
  if tool_name contains "__":
    (server_name, tool_name) = split(tool_name, "__")  // "claude-web__fetch" → ("claude-web", "fetch")
    
    mcp_manager.resolve_tool_name(tool_name)
      ├─ Look up in connected MCP servers
      └─ Return (server_name, tool_definition)
    
    result = mcp_manager.call_tool(server, tool, input)
      ├─ Invoke via stdio/SSE/WebSocket
      └─ Collect response
    
    Transform result:
      ├─ Extract text content
      ├─ Note any non-text (images, resources)
      └─ Wrap in ToolResult { content, is_error }
  else:
    return None (built-in tool, not MCP)
```

### Schema Injection

Before sending a request to the LLM:

```
build_request():
  
  tool_schemas = registry.schemas_json()  // built-in tools
  
  for (server_name, tools) in mcp_tool_schemas():
    mcp_schemas = mcp_tools_to_schemas(server_name, tools)
    tool_schemas.extend(mcp_schemas)
  
  request.tools = tool_schemas
```

Result: LLM sees both built-in and MCP tools with full schemas.

---

## 7. Tool Context Utilities

Tools access shared context via `ToolContext`:

### File State Tracking

```rust
pub struct FileStateTracker {
    // Tracks mtime, size of files to detect external modifications
}

pub fn check_file_state(&self, path: &Path) -> OxiResult<FileState> {
    // Detects: file deleted, modified externally, stale
}
```

Used by `file_edit` to prevent overwriting changes.

### Task Manager

```rust
pub struct TaskManager {
    tasks: HashMap<String, Task>,
}

pub fn create_task(&mut self, spec: TaskSpec) -> String {
    // Creates background task, returns task_id
}

pub fn update_task(&mut self, task_id: &str, update: TaskUpdate) {
    // Update status, notes, owner
}
```

Used by `task_create`, `task_list`, `task_update` tools.

### Bash Process Tracking

```rust
pub type BashProcessMap = Arc<Mutex<HashMap<String, BashProcess>>>;

pub struct BashProcess {
    pub pid: u32,
    pub command: String,
    pub started_at: Instant,
}
```

Updated by `bash` and `bash_background`, consumed by `kill_bash`.

### MCP Manager

```rust
pub struct McpServerManager {
    servers: HashMap<String, McpServer>,
}

pub async fn call_tool(
    &self,
    server: &str,
    tool: &str,
    input: Value,
) -> OxiResult<ToolResponse> {
    // Route to stdio/SSE/WebSocket transport
}
```

Used for all MCP tool execution.

---

## 8. Performance Considerations

### Streaming Tools

`bash`, `bash_background`, `web_fetch` support streaming:

- Results returned incrementally (avoid buffering entire output)
- Kill on drop: child process killed if executor drops the future
- Timeout: configurable per tool (default 300s)

### Large Result Handling

Tools with potentially large output:

| Tool | Max Recommended | Handling |
|------|-----------------|----------|
| `file_read` | 500 KB | Truncate or error if larger |
| `grep` | 200 KB | Limit matches, apply snip_compact |
| `glob` | 100 KB | Limit matches, truncate |
| `web_fetch` | 100 KB | Truncate response, summarize |
| `web_search` | 50 KB | Return only top results |

If budget is critical (L2+), compaction strategies further reduce size.

### Concurrency

- **Sequential tool execution** (current): Tools run one-by-one, results collected in order
- **Future parallelization**: Would require independent `task_id`, file locking, careful result ordering

---

## 9. Error Handling

Tools return `OxiResult<ToolResult>`:

| Outcome | Representation | LLM Sees |
|---------|-----------------|----------|
| Success | `Ok(ToolResult { content, is_error: false })` | Output as-is |
| Tool error | `Ok(ToolResult { content, is_error: true })` | Error message, can retry |
| Executor error | `Err(OxiError::Tool { ... })` | "Tool error: {e}" |
| Permission denied | `Ok(ToolResult { content: "Permission denied: {reason}", is_error: true })` | User refused |
| Timeout | `Ok(ToolResult { content: "Command timed out", is_error: true })` | Can retry or escalate |

Best practice: **Always wrap user-facing errors in `ToolResult::error()`** rather than propagating `Err`. This lets the LLM decide whether to retry.

---

## 10. Integration Points

→ See [02-query-engine.md](./02-query-engine.md) for tool dispatch from QueryEngine  
→ See [04-permission-system.md](./04-permission-system.md) for permission checking  
→ See [07-mcp-integration.md](./07-mcp-integration.md) for MCP tool details  
→ See [05-state-management.md](./05-state-management.md) for StateStore integration

---

**Document Info**  
Lines: ~750 | Tools documented: 49 | Last updated: 2026-04-12 | Version: 1.0
