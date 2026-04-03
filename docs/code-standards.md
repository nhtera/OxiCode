# OxiCode — Code Standards & Guidelines

**Version:** 0.1.0 | **Last Updated:** 2026-04-02 | **Applies to:** All 16 crates

## Core Principles

```
YAGNI (You Aren't Gonna Need It)
 ↓
Don't build features speculatively.
Build only what's needed for current requirements.

KISS (Keep It Simple, Stupid)
 ↓
Prefer obvious code over clever code.
Optimize for readability first, performance second.

DRY (Don't Repeat Yourself)
 ↓
Extract common patterns into reusable functions/traits.
But don't over-abstract; see YAGNI.
```

---

## Rust Naming Conventions

### Files & Directories
- **Module files:** `snake_case.rs` (e.g., `budget_manager.rs`, `query_engine.rs`)
- **Directories:** `snake_case/` (e.g., `src/providers/`, `src/tools/`)
- **Crates:** `kebab-case` (e.g., `oxicode-context`, `oxicode-agents`)
- **Test files:** `{module}_tests.rs` (e.g., `budget_tests.rs`)

**Goal:** Self-documenting names that clearly describe purpose when listed.

### Types & Traits
```rust
// Structs, enums, traits: PascalCase
pub struct BudgetManager { }
pub enum PermissionDecision { }
pub trait ToolExecutor { }

// Type aliases: PascalCase (follow convention)
pub type OxiResult<T> = Result<T, OxiError>;
pub type EventStream = Pin<Box<dyn Stream<Item = OxiResult<StreamEvent>> + Send>>;
```

### Variables & Functions
```rust
// Variables, fields: snake_case
let token_count = 1024;
let mut messages = Vec::new();

pub struct Message {
    pub message_id: String,
    pub created_at: DateTime<Utc>,
}

// Functions: snake_case
pub fn count_tokens(text: &str) -> usize { }
pub async fn execute_query(query: &str) -> OxiResult<String> { }
```

### Constants
```rust
// Constants: SCREAMING_SNAKE_CASE
pub const MAX_TOOL_TURNS: u32 = 50;
pub const DEFAULT_MODEL: &str = "claude-3-5-sonnet-20241022";
pub const TOKEN_RATIO_L3_TRIGGER: f64 = 0.70;
```

---

## File Organization

### Standard Module Structure

```
oxicode-context/src/
├── lib.rs                    # Re-exports public API
├── budget.rs                 # BudgetManager, BudgetStatus
├── token_counter.rs          # TokenCounter
├── truncation.rs             # truncate_messages()
├── microcompact.rs           # microcompact_messages()
├── auto_compact.rs           # AutoCompactor
├── reactive_compact.rs       # ReactiveCompactor
├── context_collapse.rs       # ContextCollapse
└── tests/
    ├── budget_tests.rs
    ├── token_counter_tests.rs
    └── ...
```

**Pattern:** One logical concept per file, keep files <300 LOC.

### lib.rs Re-exports

```rust
// GOOD: Clear public API
pub mod token_counter;
pub mod budget;
pub mod truncation;

pub use budget::{BudgetManager, BudgetStatus};
pub use token_counter::TokenCounter;
pub use truncation::truncate_messages;

// Users of crate: oxicode_context::{BudgetManager, TokenCounter}
```

**Pattern:** Expose key types at crate level, hide implementation modules.

### Error Handling in lib.rs

```rust
// Re-export OxiError from oxicode-common
pub use oxicode_common::{OxiError, OxiResult};
```

---

## Code Quality Standards

### 1. No Panics in Production

**FORBIDDEN:**
```rust
// ❌ NEVER in prod code
let value = vec![1, 2, 3];
let x = value[10];  // panic!

let result = risky_call().unwrap();  // panic if Err

let Some(x) = opt else { panic!("expected Some") };
```

**ALLOWED:**
```rust
// ✓ In tests only
#[cfg(test)]
fn test_something() {
    let value = vec![1, 2, 3];
    assert_eq!(value[0], 1);  // panics on failure, that's ok in tests
}

// ✓ In prod code: handle errors gracefully
let value = vec![1, 2, 3];
let x = value.get(10).copied().unwrap_or(0);

let result = match risky_call() {
    Ok(x) => x,
    Err(e) => {
        tracing::error!("call failed: {}", e);
        return Err(e);
    }
};

let x = opt.unwrap_or_else(|| {
    tracing::warn!("expected Some, got None");
    default_value()
});
```

### 2. Error Type Usage

**Use `OxiResult<T>` for:**
- All async functions that might fail
- All public APIs
- Any function that propagates errors

```rust
// ✓ GOOD
pub async fn execute_query(&self, query: &str) -> OxiResult<String> {
    // ...
}

pub fn validate_path(path: &str) -> OxiResult<PathBuf> {
    // ...
}

// ❌ AVOID (unless internal, infallible, or test)
pub fn get_value(&self) -> Option<String> {  // Only if truly optional
pub fn parse_json(&self) -> String {         // Only if infallible
```

**Map errors to `OxiError`:**
```rust
// ❌ WRONG
let file = std::fs::read_to_string("config.toml")?;

// ✓ GOOD
let file = std::fs::read_to_string("config.toml")
    .map_err(|e| OxiError::Config(format!("read config: {}", e)))?;

// Or use anyhow::Context (transitional)
let file = std::fs::read_to_string("config.toml")
    .context("failed to read config.toml")?;
```

### 3. Logging Standards

**Use `tracing` macros for all logging:**

```rust
use tracing::{trace, debug, info, warn, error};

// Entering a function or start of operation
debug!("starting query execution for model={}", model);

// Important state changes
info!("context defense triggered at ratio=0.95");

// Potential problems (recoverable)
warn!("tool execution failed, retrying: {}", error);

// Errors that affect user
error!("failed to spawn agent: {}", error);

// Detailed debugging (only in dev builds)
trace!("token_count={}, ratio={:.2}%", tokens, ratio * 100.0);
```

**Never use `println!` in library code:**
```rust
// ❌ AVOID
println!("Starting execution");

// ✓ GOOD
debug!("Starting execution");
```

### 4. Documentation Standards

**Public API: Doc comments required**

```rust
/// Counts tokens in text using a heuristic (1 token ≈ 4 chars).
///
/// # Arguments
/// * `text` - Input text to count
///
/// # Returns
/// Number of tokens (rough estimate, not from actual tokenizer)
///
/// # Examples
/// ```
/// use oxicode_context::TokenCounter;
/// let counter = TokenCounter::new();
/// assert!(counter.count_text("hello world") > 0);
/// ```
pub fn count_tokens(&self, text: &str) -> usize {
    text.len() / 4
}

/// Represents the current budget status of the context.
#[derive(Debug, Clone, Copy)]
pub enum BudgetStatus {
    /// Token usage < 70% of budget
    Healthy,
    /// Token usage 70-90% of budget
    Warning,
    /// Token usage 90-95% of budget
    Danger,
    /// Token usage >= 95% of budget
    Critical,
}
```

**Private/Internal: Brief comments only**

```rust
// Scan directory for SKILL.md files, skip hidden dirs
fn discover_skills(dir: &Path) -> OxiResult<Vec<SkillInfo>> {
    // Implementation...
}
```

**Complex Logic: Explain WHY, not WHAT**

```rust
// ❌ Restates code
loop {
    tokens_used += count_tokens(&msg.content);  // Add tokens
}

// ✓ Explains intent
// L1 truncation: remove oldest middle messages to stay under token budget.
// Keep first message (context) + last 3 (recent), remove middle.
loop {
    if should_remove_middle(&messages) {
        messages.drain(1..messages.len() - 3);
    }
}
```

### 5. Testing Standards

**Test naming:**
```rust
#[test]
fn test_budget_manager_returns_healthy_under_70_percent() {
    // Test names are descriptive, state the condition
}

#[test]
fn test_truncation_preserves_first_and_last_messages() {
    // Clear what's being tested
}
```

**Test structure (Arrange-Act-Assert):**
```rust
#[test]
fn test_skill_discovery_finds_skill_md_files() {
    // Arrange: Set up test data
    let temp_dir = TempDir::new().unwrap();
    let skill_file = temp_dir.path().join("SKILL.md");
    fs::write(&skill_file, "---\nname: test\n---\nPrompt").unwrap();
    
    // Act: Perform the action
    let discovery = SkillDiscovery::new(temp_dir.path().into(), PathBuf::new());
    let skills = discovery.discover().unwrap();
    
    // Assert: Verify the result
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "test");
}
```

**Test edge cases:**
```rust
#[test]
fn test_empty_input() { }

#[test]
fn test_zero_budget() { }

#[test]
fn test_missing_file() { }

#[test]
fn test_malformed_json() { }

#[test]
fn test_concurrent_access() { }
```

### 6. Async Patterns

**Always use `tokio` for async work:**

```rust
// ✓ GOOD: Consistent async runtime
pub async fn execute_turn(&self, query: &str) -> OxiResult<String> {
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        self.fetch_from_llm(query),
    ).await?;
    Ok(result)
}

// Spawning background tasks
tokio::spawn(async {
    if let Err(e) = long_running_task().await {
        error!("background task failed: {}", e);
    }
});

// Selecting between futures
tokio::select! {
    res = stream.next() => {
        match res {
            Some(event) => handle_event(event),
            None => break,
        }
    }
    _ = timeout_at(deadline) => {
        warn!("timeout waiting for stream");
    }
}
```

**Never block in async context:**
```rust
// ❌ WRONG: Blocks async executor
pub async fn fetch_data(&self) -> OxiResult<String> {
    let result = std::thread::sleep(Duration::from_secs(1));  // BLOCKS EXECUTOR
    Ok("data".to_string())
}

// ✓ GOOD: Async-aware
pub async fn fetch_data(&self) -> OxiResult<String> {
    tokio::time::sleep(Duration::from_secs(1)).await;
    Ok("data".to_string())
}
```

### 7. Trait Implementation

**Async traits require `async_trait`:**

```rust
use async_trait::async_trait;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    
    async fn execute(
        &self,
        input: serde_json::Value,
    ) -> OxiResult<ToolResult>;
}

// Implementation
pub struct FileReadTool;

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }
    
    async fn execute(
        &self,
        input: serde_json::Value,
    ) -> OxiResult<ToolResult> {
        // Async work here
        Ok(ToolResult::default())
    }
}
```

**Send + Sync bounds for thread safety:**

```rust
// ✓ GOOD: Explicit thread-safety requirements
pub struct StateStore {
    tx: Arc<RwLock<AppState>>,  // Arc is Send+Sync, RwLock is Send+Sync
}

// If type isn't Send+Sync, document why
pub struct LocalState {
    // NOT Send+Sync: contains Rc (single-threaded)
    value: Rc<String>,
}
```

### 8. Serialization

**Use `serde` with derive:**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub model: String,
    #[serde(default)]
    pub max_tokens: u32,
}

// For custom serialization logic:
impl Serialize for CustomType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Custom logic
    }
}
```

**Validate after deserialization:**

```rust
impl AgentConfig {
    pub fn validate(&self) -> OxiResult<()> {
        if self.name.is_empty() {
            return Err(OxiError::Config("name required".into()));
        }
        if self.max_tokens == 0 || self.max_tokens > 200_000 {
            return Err(OxiError::Config("max_tokens must be 1-200000".into()));
        }
        Ok(())
    }
}

// Usage
let config: AgentConfig = serde_json::from_str(json_str)?;
config.validate()?;
```

---

## Project Structure Standards

### Dependency Management

**Workspace dependencies (Cargo.toml):**

```toml
[workspace]
members = [
    "crates/oxicode-common",
    "crates/oxicode-context",
    # ...
]

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
tracing = "0.1"

# Internal crates
oxicode-common = { path = "crates/oxicode-common" }
oxicode-context = { path = "crates/oxicode-context" }
```

**Crate dependencies (individual Cargo.toml):**

```toml
[package]
name = "oxicode-context"
version.workspace = true
edition.workspace = true

[dependencies]
oxicode-common = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
tracing = { workspace = true }

# Avoid adding crates without consensus
```

**Adding new dependencies:**
1. Discuss in design phase
2. Prefer `workspace.dependencies` for shared ones
3. Keep count minimal (current: ~30 transitive deps)

### Feature Flags

```toml
[features]
default = ["compression"]
compression = []
telemetry = []
debug-logs = []
```

```rust
#[cfg(feature = "compression")]
pub mod compression {
    // Only compiled with --features=compression
}
```

---

## Performance Guidelines

### Micro-optimizations: No

```rust
// ❌ Premature optimization
let tokens = text.chars().count() / 4;  // Slower than len()/4

// ✓ Readable, fast enough
let tokens = text.len() / 4;
```

### Macro-level: Yes

```rust
// ❌ Inefficient: O(n²) because Vec::remove(0) shifts all
fn slow_remove_first(v: &mut Vec<i32>) {
    for _ in 0..10 {
        v.remove(0);  // Shifts n-1 elements each time
    }
}

// ✓ Efficient: O(n)
fn fast_remove_first(v: &mut Vec<i32>) {
    v.drain(0..10);  // Single pass
}
```

### Allocation Aware

```rust
// ❌ Allocates in loop
let mut results = Vec::new();
for item in items {
    let mut processed = Vec::new();  // Allocated 1000x
    processed.push(process(item));
    results.extend(processed);
}

// ✓ Pre-allocated
let mut results = Vec::with_capacity(items.len());
for item in items {
    results.push(process(item));
}
```

### Avoid Excessive Cloning

```rust
// ❌ Clones on every iteration
for msg in messages.iter() {
    let msg_copy = msg.clone();  // Expensive for large Message
    handle_message(msg_copy);
}

// ✓ Borrow
for msg in &messages {
    handle_message(msg);
}

// ✓ If move needed
for msg in messages.into_iter() {
    handle_message(msg);
}
```

---

## Clippy & Linting

### Required Checks

```bash
# Run before commit
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
cargo test --all
```

### Configuration (Cargo.toml)

```toml
[workspace.lints.rust]
unsafe_code = "forbid"  # Absolutely no unsafe

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }

# Allowed exceptions (common patterns)
module_name_repetitions = "allow"
must_use_candidate = "allow"
missing_errors_doc = "allow"
doc_markdown = "allow"
```

**Never suppress lints without comment:**
```rust
// ❌ WRONG
#[allow(clippy::too_many_arguments)]
pub fn foo(a: i32, b: i32, c: i32, d: i32) { }

// ✓ GOOD
// Multiple args justified: each is independent parameter
#[allow(clippy::too_many_arguments)]
pub fn foo(a: i32, b: i32, c: i32, d: i32) { }
```

---

## Git & Commit Standards

### Conventional Commits

```
feat: add context defense layer 4 (reactive compaction)
fix: divide by zero in BudgetManager when max_tokens is 0
docs: update system architecture for Phase 4
refactor: split QueryEngine into smaller modules
test: add edge case tests for empty input
chore: update dependencies
```

### Commit Message Format

```
<type>: <short summary (50 chars max)>

<detailed explanation (optional, wrap at 72 chars)>

Fixes #123
Related: #456
```

**Types:** feat, fix, docs, refactor, test, chore, style, perf

---

## Review Checklist

Before submitting PR:

- [ ] Compiles with `cargo build --all`
- [ ] All tests pass: `cargo test --all`
- [ ] No clippy warnings: `cargo clippy --all -- -D warnings`
- [ ] Formatted: `cargo fmt --all`
- [ ] Doc comments added for public API
- [ ] No panics in production code
- [ ] Error handling uses `OxiResult`
- [ ] Logging uses `tracing` macros
- [ ] No `println!` in library code
- [ ] Unsafe code has justification + `// SAFETY:` comment
- [ ] Tests cover happy path + edge cases
- [ ] Commit messages follow conventional format

---

## Common Mistakes

| Mistake | Fix |
|---------|-----|
| Using `.unwrap()` in prod | Use `?` operator or `map_err` |
| Blocking async with `thread::sleep` | Use `tokio::time::sleep().await` |
| Cloning large data structures | Use `&` (borrow) or move semantics |
| Suppressing clippy without reason | Add comment explaining why |
| Missing error context | Use `.context()` or map to `OxiError` |
| Printing to stdout in libs | Use `tracing::info!()` |
| Writing to `tests/` without integration | Put unit tests in `src/` modules instead |
| Not validating deserialized data | Add `.validate()` method post-serde |
| Spawning tasks without error handling | Wrap in `tokio::spawn` with error logging |
| Using `Vec::remove(0)` in loop | Use `drain()` or `VecDeque` |

---

## Phase 2 Provider Integration Standards

See `provider-integration-guide.md` for detailed patterns on implementing custom LLM providers, using MessageRequest builders, and environment variable conventions.

---

## Phase 3 Gap Closure Standards

### Tool Implementation Checklist

When adding new tools (Phase 3 closure: 11 new tools added):
1. Create new tool file in `oxicode-tools/src/{tool_name}.rs`
2. Implement `Tool` trait with async `execute()` method
3. Define `ToolSchema` with name, description, input_schema
4. Register in `lib.rs` `default_registry()` function
5. Handle edge cases: empty input, missing files, permission errors
6. Return `ToolResult` with appropriate content and error status
7. Use `tracing` for tool-level debugging
8. Write unit tests covering happy path + error cases
9. Validate all JSON schema inputs before execution

### Phase 3 New Tools
- **TodoWriteTool:** Manages todo list persistence
- **Team Tools:** TeamCreate, TeamDelete for multi-agent teams
- **LSP Tool:** Language Server Protocol integration
- **PowerShell:** Cross-platform shell execution
- **REPL Tool:** Interactive Python/JS/etc REPL
- **MCP Auth:** Authentication credential management
- **Synthetic Output:** Test data generation
- **Background PR:** Automated PR suggestion
- **Verify Plan:** Plan execution validation
- **Workflow:** Task automation orchestration

---

## Phase 4 Integration Standards

### Context Defense Code

**All layers must:**
1. Return `OxiResult<()>` or similar
2. Log decisions: `debug!("L3 auto-compact triggered at ratio={:.2}%", ratio)`
3. Handle empty input gracefully
4. Not panic on edge cases

### Multi-Agent Code

**Agent spawning:**
1. Validate `AgentConfig` before spawn
2. Write config to stdin as JSON
3. Monitor child process, log on exit
4. Clean up resources (kill process if parent dies)

### Skill System Code

**Skill discovery:**
1. Handle missing/empty skill directories
2. Validate YAML frontmatter
3. Log parse errors, skip invalid skills
4. Don't follow circular symlinks

### Background Task Code

**Task execution:**
1. Spawn with proper signal handling
2. Stream output to disk as JSONL
3. Notify listeners on status change
4. Clean up temp files on completion

---

## References

- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial)
- [Serde Documentation](https://serde.rs/)
- [Tracing Documentation](https://docs.rs/tracing/)
- [Clippy Lints](https://doc.rust-lang.org/clippy/)

