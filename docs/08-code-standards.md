# Code Standards

Comprehensive code standards and conventions for the OxiCode Rust workspace. All code must adhere to these standards to maintain consistency, performance, reliability, and security across 17 crates and ~51,600 LOC.

---

## 1. Toolchain & Workspace Setup

**Edition:** Rust 2021 (stable features; experimental work gated behind feature flags).

**MSRV:** 1.80 (Minimum Supported Rust Version). All code must compile with `rustc 1.80+`.

```bash
# Verify MSRV
cargo +1.80 check --workspace
```

**Unsafe Code:** `unsafe_code = "forbid"` workspace-wide. **No `unsafe` blocks allowed** except in vendored dependencies. This is enforced via:

```toml
# Cargo.toml [workspace.lints.rust]
unsafe_code = "forbid"
```

**Clippy Lints:** All crates enforce `clippy::all` + `clippy::pedantic` at warn level, with 14 curated exceptions:

```toml
[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"      # Tool trait re-exports are idiomatic
must_use_candidate = "allow"           # Not all fallible ops need #[must_use]
missing_errors_doc = "allow"           # Documented in narrative sections
missing_panics_doc = "allow"           # Panics from ? propagation are clear
return_self_not_must_use = "allow"     # Builder patterns common in CLI
struct_field_names = "allow"           # Field names intentionally repeat type
enum_variant_names = "allow"           # Variant names use consistent prefix
unused_self = "allow"                  # Some pattern matches need all bindings
unnecessary_literal_bound = "allow"    # MSRV 1.80 compatibility
needless_pass_by_value = "allow"       # Arc/owned types sometimes preferred
cast_possible_truncation = "allow"     # Checked manually in performance paths
doc_markdown = "allow"                 # Code snippets need flexibility
```

**CI Enforcement:** GitHub Actions runs `RUSTFLAGS="-D warnings"` — all lints become hard errors.

(source: Cargo.toml:23–48)

---

## 2. Error Handling

### OxiResult / OxiError Pattern

All library code uses `OxiResult<T>` (alias for `Result<T, OxiError>`) from `oxicode-common`. CLI code may use `anyhow::Result<T>` for ergonomic error chaining at entry points.

```rust
// Library code (oxicode-* crates)
pub fn do_work() -> OxiResult<String> {
    let data = some_fallible_op()?;  // Propagate via ?
    Ok(data)
}

// CLI code (oxicode-cli main)
async fn main() -> anyhow::Result<()> {
    let result = library_call()?;  // Still uses ?
    Ok(())
}
```

### OxiError Variants

All 11 error variants from `oxicode-common/src/error.rs`:

| Variant | Usage |
|---------|-------|
| `Api { message, status, retryable }` | HTTP/LLM provider failures; retryable auto-detected for 429/500/502/503/529 |
| `RateLimit { info: RateLimitInfo }` | Dedicated variant for rate limit events (always retryable) |
| `Config(String)` | Invalid configuration or missing settings |
| `Tool { name, message }` | Tool-specific failures (bash, file ops, etc.) |
| `Permission(String)` | Permission pipeline denial reasons |
| `Session(String)` | Session load/save or state corruption |
| `Io(std::io::Error)` | File/network I/O (implements From trait) |
| `Json(serde_json::Error)` | JSON serialization (implements From trait) |
| `Tui(String)` | Terminal UI rendering failures |
| `StreamClosed` | Unexpected stream termination |
| `Other(String)` | Catch-all for unmapped errors |

### Error Construction

```rust
// Direct construction
OxiError::config("Missing API key")
OxiError::Permission("File write denied".into())

// With HTTP status (retryability auto-detected)
OxiError::api_with_status("Rate limited", 429)        // retryable = true
OxiError::api_with_status("Bad request", 400)         // retryable = false

// From conversion (std::io::Error, serde_json::Error)
file_op().map_err(|e| OxiError::from(e))?
```

### Error Context (map_err)

Add context before propagating:

```rust
fs::read_to_string(path)
    .map_err(|e| {
        tracing::error!(file = %path, "Failed to read config");
        OxiError::Io(e)
    })?;
```

### Retryability Detection

```rust
if error.is_retryable() {
    // Implement backoff (exponential: 100ms → 200ms → 400ms)
    retry_with_backoff(operation, 3).await?
} else {
    // Log and fail fast
    return Err(error);
}
```

**Safe status codes:** 429 (rate limit), 500 (server error), 502 (bad gateway), 503 (unavailable), 529 (overloaded). All others are non-retryable.

### Crate-Specific Errors

Service crates define custom error types via `thiserror`, then convert to `OxiError`:

```rust
// crates/oxicode-tools/src/tool_error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("Tool '{name}' not found")]
    NotFound { name: String },
    
    #[error("Tool execution failed: {0}")]
    Execution(String),
}

// Convert to OxiError in public API boundary
impl From<ToolError> for OxiError {
    fn from(err: ToolError) -> Self {
        OxiError::Tool {
            name: "unknown".into(),
            message: err.to_string(),
        }
    }
}
```

(source: crates/oxicode-common/src/error.rs)

---

## 3. Async Patterns

### Tokio Runtime

All async code uses `tokio` with full feature set enabled workspace-wide:

```toml
[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
```

This enables:
- `tokio::spawn()` — task spawning (work-stealing scheduler)
- `tokio::sync::*` — channels (watch, mpsc, oneshot, broadcast)
- `tokio::time::*` — timeouts, intervals, sleep
- `tokio::fs::*` — async file I/O
- `tokio::net::*` — async TCP/UDP sockets
- `tokio::task::block_in_place()` — blocking operations in async context

### Shared State (Arc)

Use `Arc<T>` for multi-threaded shared ownership. Clone before spawning tasks. **Rule:** Never move raw data into a task; wrap in `Arc` first.

### Mutex Strategy

- **Sync-only:** `std::sync::Mutex<T>` — no async contention, simpler
- **Async code:** `tokio::sync::Mutex<T>` — safe across `.await` points
- **Critical rule:** Never hold `std::sync::Mutex` across `.await` — can deadlock

(source: crates/oxicode-core/src/query_engine.rs:11–58)

### Channel Patterns

| Channel | Pattern | Use Case |
|---------|---------|----------|
| `watch` | 1-sender, N-receiver, latest value | StateStore broadcast |
| `mpsc` | N-sender, 1-receiver, ordered queue | UiEvent/CoreEvent |
| `oneshot` | 1-sender, 1-receiver, single reply | Permission dialog responses |
| `broadcast` | 1-sender, N-receiver, fanout | Minimal use |

(source: crates/oxicode-state/src/lib.rs, crates/oxicode-cli/src/main.rs)

### Cancellation Patterns

Use `tokio::select!` with `AtomicBool` cancel flags or timeouts:

```rust
tokio::select! {
    result = stream.next() => { /* process */ }
    _ = cancel_poll() => { /* interrupted */ }
}
```

---

## 4. Module Organization

**One logical unit per file.** Prefer narrow file scope over god files.

**Pattern:**
```
crates/oxicode-tools/src/
├── lib.rs                # Re-export public API
├── tool_trait.rs         # Tool trait definition
├── tool_registry.rs      # Registry + dispatch
├── bash.rs               # `bash` tool implementation
├── file_read.rs          # `file_read` tool
├── file_write.rs         # `file_write` tool
├── grep.rs               # `grep` tool
└── ... (49 total tools)
```

**lib.rs strategy:** Re-export public API only, hide implementation details.

```rust
// crates/oxicode-tools/src/lib.rs
pub mod tool_trait;
pub mod tool_registry;
mod bash;  // Private, re-export via registry
mod grep;  // Private

pub use tool_registry::{Tool, ToolRegistry, ToolContext, ToolResult};
pub use tool_trait::ToolMetadata;
```

**Feature gates:** Use `#[cfg(feature = "...")]` for optional functionality.

```rust
#[cfg(feature = "voice")]
pub mod voice_input;

#[cfg(feature = "bridge")]
pub mod bridge_transport;
```

**Crate boundary rules:**
- Internal `oxicode-*` crates are path deps (Cargo.toml)
- Cyclic dependencies forbidden (use traits to break cycles)
- Each crate owns its error type (or re-exports OxiError)
- No secrets in crates — secrets belong in CLI layer only

(source: Cargo.toml:1–20, crates/*/src/lib.rs)

---

## 5. Naming Conventions

### Types (PascalCase)

```rust
pub struct QueryEngine { }
pub struct ToolRegistry { }
pub struct StateStore { }
pub struct PermissionPipeline { }
pub enum ContentBlock { }
pub trait LlmProvider { }
pub trait Tool { }
```

### Functions & Methods (snake_case)

```rust
pub fn execute_turn() { }
pub async fn stream_message() { }
fn check_budget() { }
pub fn max_tokens(&self) -> u32 { }
async fn apply_defense(&mut self, msgs: &[Message]) -> OxiResult<()> { }
```

### Constants (SCREAMING_SNAKE_CASE)

```rust
const MAX_TOOL_TURNS: usize = 50;
const DEFAULT_MODEL_MAX_TOKENS: usize = 200_000;
const LOG_ROTATION_SIZE: u64 = 10 * 1024 * 1024;  // 10 MB
const RATE_LIMIT_BACKOFF_MS: u64 = 100;
```

### Crates (kebab-case)

```
oxicode-cli              ← Binary entry point
oxicode-core             ← Query engine
oxicode-api              ← LLM provider abstraction
oxicode-tools            ← 49 built-in tools
oxicode-permissions      ← Permission pipeline
oxicode-tui              ← Terminal UI (ratatui)
oxicode-state            ← Centralized app state
oxicode-config           ← Configuration loading
oxicode-session          ← Session persistence
oxicode-common           ← Shared types & errors
```

### Modules (snake_case)

```rust
mod query_engine;
mod tool_registry;
mod stream_event;
pub use query_engine::QueryEngine;
```

### Files (snake_case.rs)

```
query_engine.rs
tool_registry.rs
budget_manager.rs
permission_pipeline.rs
```

### Trait Implementations

Store `impl` blocks for external types in separate files:

```rust
// src/conversions.rs
impl From<ToolError> for OxiError { }
impl From<ConfigError> for OxiError { }
```

(source: Cargo.toml members, crates/*/src/)

---

## 6. Testing

### Unit Tests (Inline)

Place tests in the same file, gated with `#[cfg(test)]`:

```rust
// crates/oxicode-common/src/error.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_429_status_creates_retryable_error() {
        let err = OxiError::api_with_status("Rate limited", 429);
        assert!(err.is_retryable());
    }

    #[test]
    fn test_400_status_not_retryable() {
        let err = OxiError::api_with_status("Bad request", 400);
        assert!(!err.is_retryable());
    }
}
```

### Integration Tests

Place in `tests/` directory at crate root:

```
crates/oxicode-permissions/tests/
├── live_permission_pipeline.rs
└── live_api_integration.rs
```

```rust
// tests/live_permission_pipeline.rs
#[tokio::test]
async fn test_permission_pipeline_denies_dangerous_commands() {
    let pipeline = PermissionPipeline::new(...);
    let result = pipeline.check("rm -rf /").await;
    assert_eq!(result, Decision::Deny);
}
```

### Test Coverage

~191 unit tests across:
- Error handling (OxiError variants, retryability)
- Permission pipeline (safe allowlist, dangerous pattern detection)
- Tool registry (dispatch, caching)
- State mutations (concurrent updates)
- Session persistence (save/load)
- Message serialization (roundtrip fidelity)

### Running Tests

```bash
# All tests in workspace
cargo test --workspace

# Single crate
cargo test --package oxicode-core

# Single test by name
cargo test --package oxicode-common test_429_status

# With output
cargo test --workspace -- --nocapture

# With backtrace
RUST_BACKTRACE=1 cargo test --workspace
```

### Test Helpers

Define reusable test fixtures:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_app() -> QueryEngine {
        QueryEngine::new(
            Arc::new(MockProvider::new()),
            Arc::new(StateStore::new(AppState::default())),
            Arc::new(ToolRegistry::new()),
            Arc::new(PermissionPipeline::default()),
            ToolContext::default(),
            "test-model".into(),
            4000,
            "test prompt".into(),
        )
    }

    #[tokio::test]
    async fn test_query_engine_handles_tool_errors() {
        let app = make_test_app();
        // test logic
    }
}
```

### Mock Provider for API Tests

```rust
pub struct MockProvider {
    responses: Vec<StreamEvent>,
}

#[async_trait]
impl LlmProvider for MockProvider {
    async fn stream_message(&self, _: MessageRequest) -> OxiResult<EventStream> {
        // Return canned responses for testing
        Ok(EventStream::from(self.responses.clone()))
    }
}
```

(source: crates/oxicode-common/src/error.rs:89–207, various tests/)

---

## 7. Documentation

### Public API (/// comments)

All `pub` items require doc comments. Include usage examples for complex types:

```rust
/// Multi-turn query engine with tool execution and budget tracking.
///
/// # Example
/// ```no_run
/// let engine = QueryEngine::new(provider, state_store, registry, ...);
/// let response = engine.execute_turn(user_input).await?;
/// ```
pub struct QueryEngine { }
```

### Module Documentation (//!)

Document entire modules at the top with `//!` comments explaining purpose and relationships.

### Complex Flows

Include ASCII diagrams in doc comments for non-obvious logic (multi-step flows, state machines).

### Inline Comments

Comment non-obvious logic only — avoid narrating code that reads clearly. Design docs go in `docs/`, not code comments.

(source: crates/oxicode-core/src/query_engine.rs, crates/oxicode-permissions/src/pipeline.rs)

---

## 8. Dependencies

All crates share dependency versions via `[workspace.dependencies]` in root `Cargo.toml`. Crates reference dependencies without version pinning:

```toml
# Cargo.toml
[dependencies]
tokio = { workspace = true }
serde = { workspace = true }
```

**Minimize feature flags** to reduce binary bloat and compile time:

```toml
# ✓ Lean — only needed features
reqwest = { version = "0.12", features = ["json", "stream", "rustls-tls"], default-features = false }

# ✗ Avoid — bloats binary with unused features
reqwest = { version = "0.12" }  # Enables all default features (native-tls, gzip, etc.)
```

### Key Dependencies

| Crate | Purpose | Version |
|-------|---------|---------|
| tokio | Async runtime | 1.x |
| serde + serde_json | Serialization | 1.x |
| reqwest | HTTP client | 0.12 |
| ratatui | Terminal UI | 0.29 |
| crossterm | Terminal control | 0.28 |
| thiserror | Error derives | 2.x |
| anyhow | Ergonomic errors (CLI only) | 1.x |
| tracing | Structured logging | 0.1 |
| chrono | Date/time | 0.4 |
| uuid | Unique IDs | 1.x |
| rmcp | MCP protocol | 1.3 |
| tokio-tungstenite | WebSocket | 0.24 |
| futures + tokio-stream | Stream utilities | 0.3, 0.1 |
| clap | CLI argument parsing | 4.x |
| git2 | Git operations | 0.19 |

### Transport & Encoding

| Crate | Purpose |
|-------|---------|
| base64 | Encoding |
| pulldown-cmark | Markdown parsing |
| syntect | Syntax highlighting |
| flate2 + tar | Archive extraction |
| glob + walkdir | Filesystem traversal |
| regex + ignore | Pattern matching |

(source: Cargo.toml:50–156)

---

## 9. Performance

### Release Profiles

Three profiles tailored for different use cases:

**Production (release):**
```toml
[profile.release]
lto = "thin"          # Faster builds, good size reduction
strip = true          # Remove debug symbols (~30% smaller)
codegen-units = 1     # Maximum optimization (slower build)
opt-level = 3         # Full speed optimization
panic = "abort"       # Smaller binary (no unwinding tables)
```
Result: ~30 MB binary, ~2 min compile time.

**Embedded/Container (release-small):**
```toml
[profile.release-small]
inherits = "release"
opt-level = "z"       # Optimize for size over speed
lto = "fat"           # Maximum size reduction (~5% smaller)
```
Result: ~20 MB binary, ~4 min compile time.

**Profiling (with debug info):**
```toml
[profile.profiling]
inherits = "release"
strip = false         # Keep debug symbols
debug = true          # Include line number info
```
Result: Used with `perf` or `flamegraph` for benchmarking.

(source: Cargo.toml:160–177)

### Hot Path Guidelines

**Avoid allocations in tight loops:**
```rust
// ✗ Bad — allocates per iteration
for item in items {
    let formatted = format!("Item: {}", item);
    process(&formatted);
}

// ✓ Good — reusable buffer
let mut buf = String::new();
for item in items {
    buf.clear();
    buf.push_str("Item: ");
    buf.push_str(&item.to_string());
    process(&buf);
}
```

**Cache expensive lookups:**
```rust
// ✓ BudgetManager caches model max_tokens to avoid repeated API calls
let max = budget.get_or_fetch_model_max_tokens("claude-opus").await?;
```

**Batch I/O operations:**
```rust
// ✓ Read entire file at once, not line-by-line
let content = fs::read_to_string(path)?;
process_lines(&content);
```

---

## 10. CI/CD Pipeline

### Local Pre-commit Checks

Run these before committing:

```bash
# Format check
cargo fmt --all --check

# Lint (warnings as errors)
cargo clippy --workspace -- -D warnings

# Test
cargo test --workspace

# Type check
cargo check --workspace
```

### GitHub Actions Order

1. **Format** — `cargo fmt --all --check` (fail fast)
2. **Clippy + Test** — Parallel on 3 OS (linux/macos/windows)
3. **Build** — 5 target triples (x86_64-linux, aarch64-linux, x86_64-macos, aarch64-macos, x86_64-windows)
4. **Build release-small** — Size-optimized profile

**CI sets:** `RUSTFLAGS="-D warnings"` → all warnings become hard errors.

### Build Targets

```bash
# Standard targets
cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-pc-windows-msvc

# Size-optimized
cargo build --release-small
```

### Test Strategy

```bash
# Fast feedback loop (local)
cargo test --workspace --lib       # Unit tests only (~3s)
cargo test --workspace             # All tests (~10s)

# CI runs full suite with all features
cargo test --workspace --all-features
```

---

## 11. Security & Hardening

### Forbidden Patterns

| Pattern | Reason | Safe Alternative |
|---------|--------|-------------------|
| `unsafe { }` | Memory safety | Use safe abstractions (Arc, Mutex, etc.) |
| `unwrap()` in library | Panics in prod | Use `?`, `map_err()`, or `OxiResult<T>` |
| Hardcoded secrets | Credential leaks | Use env vars + secure config files |
| String interpolation for commands | Shell injection | Use `Command::new()` with args slice |
| Dynamic SQL strings | SQL injection | Use parameterized queries (not applicable here) |

### Safe Patterns

**Mutex error handling:**
```rust
// ✗ Bad — panics on poison
let guard = mutex.lock().unwrap();

// ✓ Good — handles poison gracefully
let guard = mutex.lock()
    .map_err(|_| OxiError::Other("Mutex poisoned".into()))?;
```

**Command execution (no shell injection):**
```rust
// ✗ Bad — shell injection risk
let output = Command::new("sh")
    .arg("-c")
    .arg(format!("grep {} {}", pattern, file))  // User input in command!
    .output()?;

// ✓ Good — args are safe
let output = Command::new("grep")
    .arg(pattern)
    .arg(file)
    .output()?;
```

**File operations with permission checks:**
```rust
// All file/tool operations pass through PermissionPipeline
let decision = permission_pipeline.check_file_read(path).await?;
if decision != Decision::Allow {
    return Err(OxiError::Permission("File read denied".into()));
}
fs::read_to_string(path)?
```

---

## 12. Commit Messages

**Format:** `<type>: <description>` (lowercase, ≤70 characters)

**Types:**
- `feat:` — New feature
- `fix:` — Bug fix
- `docs:` — Documentation changes
- `refactor:` — Code refactoring (no behavior change)
- `test:` — Test additions/improvements
- `chore:` — Build, dependencies, CI config

**Examples:**
```
feat: add rate limit retry backoff exponential strategy

fix: handle permission pipeline null pointer in concurrent context

docs: expand error handling section with retry patterns

refactor: split tool_registry into registry and dispatcher modules

test: add 18 new permission pipeline edge cases
```

**Never include:**
- AI references ("Claude", "AI", etc.)
- Sensitive information (API keys, passwords)
- Unrelated changes (one concern per commit)

---

→ See [00-index.md](./00-index.md) for navigation.
