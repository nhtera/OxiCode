# Permission System Design

**Version:** 1.0  
**Last Updated:** 2026-04-12  
**Related:** `crates/oxicode-permissions/src/`, `crates/oxicode-tui/src/events.rs`

## Overview

OxiCode's permission system is a **6-layer security pipeline** that gates all tool invocations before execution. Each layer performs a specific security check, and the first matching rule determines the outcome: **Allow**, **Deny**, or **Ask** the user.

The system is designed to be:
- **Transparent:** Users understand why tools are allowed/blocked
- **Flexible:** Supports multiple permission modes and user-defined rules
- **Secure:** Hard denies for known attack patterns are NEVER bypassed

---

## Core Types

### PermissionDecision Enum

```rust
pub enum PermissionDecision {
    /// Tool execution is allowed.
    Allow,
    /// Tool execution is denied with a reason.
    Deny(String),
    /// User must be prompted for approval.
    Ask(String),
}
```

The pipeline always returns one of these three states.

(source: `crates/oxicode-permissions/src/pipeline.rs`)

---

### PermissionMode Enum

```rust
pub enum PermissionMode {
    /// Normal mode — proceed through all layers (default).
    Default,
    /// Bypass all permission checks except hard denies.
    Bypass,
    /// Always ask for approval on non-readonly tools.
    ApprovalOnly,
}
```

- **Default:** Evaluates all 6 layers in order
- **Bypass:** Skips dangerous pattern detection & rule matching (but NEVER skips hard denies)
- **ApprovalOnly:** Forces `Ask` for any non-readonly tool

(source: `crates/oxicode-permissions/src/pipeline.rs`)

---

### ToolPermissionLevel Enum

```rust
pub enum ToolPermissionLevel {
    ReadOnly,    // File reads, directory listing, etc.
    FileWrite,   // Create/modify files
    ShellExec,   // Execute shell commands
    System,      // System-level operations
}
```

Tools are classified by the level of access they grant.

(source: `crates/oxicode-permissions/src/pipeline.rs`)

---

### PermissionPipeline Struct

```rust
pub struct PermissionPipeline {
    mode: PermissionMode,
    rule_matcher: RuleMatcher,
    dangerous_detector: DangerousPatternDetector,
    command_security: CommandSecurityChecker,
    tracker: DenialTracker,
    /// Session-scoped tool names that user chose "Always allow" for.
    session_allowlist: Mutex<HashSet<String>>,
    /// Session-scoped tool names that user chose "Always deny" for.
    session_denylist: Mutex<HashSet<String>>,
}
```

The main struct orchestrating all permission checks. Fields:
- **mode:** Current permission mode
- **rule_matcher:** Evaluates user-defined permission rules
- **dangerous_detector:** Detects dangerous command patterns
- **command_security:** Hard-deny security patterns
- **tracker:** Records denial events for auditing
- **session_allowlist/denylist:** In-memory per-session decisions

(source: `crates/oxicode-permissions/src/pipeline.rs`)

---

## 6-Layer Permission Pipeline

### ASCII Flow Diagram

```
User invokes tool with input
         ↓
    [Layer 1: ReadOnly Check]
         ↓
    ALLOW? YES → Return Allow
         ↓ NO
    [Layer 2: Command Security Hard-Deny]
         ↓
    DENY? YES → Return Deny (NEVER bypassed)
         ↓ NO
    [Layer 3: Dangerous Pattern Detection]
         ↓
    ASK/ALLOW? YES → In Bypass mode? YES → Allow + warn
                     In Bypass mode? NO  → Ask
         ↓ NO
    [Layer 4: Session Allowlist/Denylist]
         ↓
    CACHED? YES → Return cached decision
         ↓ NO
    [Layer 5: Permission Mode Branch]
         ↓
    Bypass mode?   YES → Allow
    ApprovalOnly?  YES → Ask
    Default?       YES → Continue
         ↓
    [Layer 6: User Rule Matching + Default]
         ↓
    Rules matched? YES → Return rule decision
                   NO  → Ask (default for non-readonly)
```

### Layer 1: ReadOnly Fast Path

```
if tool_level == ToolPermissionLevel::ReadOnly {
    return Allow
}
```

Read-only tools (file_read, grep, glob, etc.) are always allowed. This is the fastest path.

**Examples:** `file_read`, `grep`, `glob`

---

### Layer 2: Command Security Hard-Deny

```
if tool_level == ToolPermissionLevel::ShellExec {
    if let Some(reason) = command_security.check(input) {
        return Deny(reason)
    }
}
```

Checks for shell-specific attack patterns that are **NEVER allowed**, even in Bypass mode:
- Zsh equals expansion (`=(`)
- Zsh module loading (`zmodload`)
- Deep path traversal (`../../../..`)
- LD_PRELOAD injection (`export LD_PRELOAD=...`)
- macOS dylib injection (`export DYLD_...`)
- Netcat listeners (`nc -l`)

**Bypass mode:** Respects this layer (cannot override)

(source: `crates/oxicode-permissions/src/command_security.rs`)

---

### Layer 3: Dangerous Pattern Detection

```
if let Some(reason) = dangerous_detector.check(tool_name, input) {
    if mode == PermissionMode::Bypass {
        warn!("Dangerous pattern in {}: {}", tool_name, reason);
        return Allow  // Ask → Allow in Bypass
    }
    return Ask(reason)
}
```

Detects risky but not necessarily exploit-level patterns. Returns `Ask` in Default/ApprovalOnly, but `Allow` (with warning) in Bypass mode.

**Dangerous Patterns by Category:**

**Filesystem:** 
- `rm -rf /` — recursive root deletion
- `rm -rf ~` — recursive home deletion
- `chmod 777` — world-writable permissions
- `/etc/passwd`, `/etc/shadow` — system files
- `~/.ssh/`, `~/.gnupg/`, `.env` — sensitive files

**Network:**
- `curl | sh`, `wget | bash` — pipe to shell
- `> /dev/sd*` — direct disk write

**Process:**
- `:(){ :|:& };:` — fork bomb
- `mkfs.` — filesystem format
- `dd if=` — direct disk copy

**Privilege:**
- `sudo` — any sudo command

(source: `crates/oxicode-permissions/src/dangerous.rs`)

---

### Layer 4: Session Allowlist/Denylist

```
if session_allowlist.contains(tool_name) {
    return Allow
}
if session_denylist.contains(tool_name) {
    return Deny(...)
}
```

Checks if the user previously chose "Always allow" or "Always deny" for this tool in the current session. These are **per-session** (not persisted between restarts).

**Important:** Session allowlist is checked AFTER dangerous patterns, so dangerous commands still trigger `Ask` even if the tool is allowlisted.

(source: `crates/oxicode-permissions/src/pipeline.rs:105-113`)

---

### Layer 5: Permission Mode Branch

```
match mode {
    PermissionMode::Bypass => return Allow,
    PermissionMode::ApprovalOnly => return Ask(...),
    PermissionMode::Default => continue to Layer 6,
}
```

Routes to the appropriate strategy based on the current mode. By this point, security checks (Layers 2-3) have already passed.

(source: `crates/oxicode-permissions/src/pipeline.rs:115-124`)

---

### Layer 6: User Rule Matching + Default

```
if let Some(decision) = rule_matcher.check(tool_name, input) {
    return decision
}
// No rule matched
return Ask("Approve {tool_name}?")
```

Evaluates user-defined permission rules from config (CLAUDE.md). If no rule matches, defaults to `Ask` for any non-readonly tool.

(source: `crates/oxicode-permissions/src/pipeline.rs:126-132`)

---

## Dangerous Pattern Detector

### Purpose
Scans tool input (shell commands, file paths) for risky patterns using regex matching.

### Key Features
- **Recursive JSON scanning:** Extracts all string values from JSON input, handles nested objects/arrays
- **Case-sensitive matching:** Patterns are case-sensitive to avoid false positives
- **Non-blocking:** Returns a reason string if dangerous; None if safe

### Implementation

```rust
pub struct DangerousPatternDetector {
    patterns: Vec<DangerousPattern>,
}

impl DangerousPatternDetector {
    pub fn check(&self, tool_name: &str, input: &serde_json::Value) -> Option<String> {
        let strings = extract_all_strings(input);  // Recursively extract strings
        let combined = strings.join(" ");
        
        for pattern in &self.patterns {
            if pattern.regex.is_match(&combined) {
                return Some(format!(
                    "Dangerous pattern detected in {}: {}",
                    tool_name, pattern.description
                ));
            }
        }
        None
    }
}
```

(source: `crates/oxicode-permissions/src/dangerous.rs`)

---

## Command Security Checker

### Purpose
Hard-deny shell attacks that exploit shell interpreter features (Zsh CVEs, LD_PRELOAD, etc.).

### Key Features
- **Attack-specific patterns:** Only flags truly exploitable patterns, not normal shell syntax
- **Zsh support:** Detects Zsh-specific attacks (equals expansion, module loading)
- **Environment injection:** Catches LD_PRELOAD and dylib injection attempts

### Patterns

```
=\(                      → Zsh equals expansion attack
zmodload                 → Zsh module loading
\.\./\.\./\.\./         → Deep path traversal
export LD_PRELOAD        → LD_PRELOAD injection
export DYLD_             → macOS dylib injection
nc -l                    → Netcat listener
ncat                     → Ncat usage
```

**Note:** Removed overly broad patterns like `$(...)` and backticks — these are common shell syntax.

(source: `crates/oxicode-permissions/src/command_security.rs`)

---

## Rule System

### PermissionRule Struct

```rust
pub struct PermissionRule {
    /// Tool name to match (can be "*" for wildcard).
    pub tool: String,
    /// Optional regex pattern to match against tool input (stringified).
    pub input_pattern: Option<String>,
    /// Action: Allow, Deny, or Ask.
    pub action: RuleAction,
}

pub enum RuleAction {
    Allow,
    Deny,
    Ask,
}
```

Rules define user policies for specific tools and input patterns.

(source: `crates/oxicode-permissions/src/rules.rs`)

---

### Rule Config Format (CLAUDE.md)

```toml
# Allow bash commands matching "cargo test"
[[permissions.rules]]
tool = "bash"
input_pattern = "cargo test"
action = "allow"

# Deny any tool with "password" in input
[[permissions.rules]]
tool = "*"
input_pattern = "password"
action = "deny"

# Ask for approval on file_write (default, but explicit)
[[permissions.rules]]
tool = "file_write"
action = "ask"
```

### Evaluation Order
- **First matching rule wins.** If no rule matches, layer 6 defaults to `Ask` for non-readonly tools
- **Wildcard `*` tool** matches any tool
- **Optional input_pattern:** If pattern is absent, the rule matches any input for that tool

(source: `crates/oxicode-permissions/src/rules.rs:77-104`)

---

## RuleMatcher

### Purpose
Evaluates tool invocations against a list of permission rules.

### Implementation

```rust
pub struct RuleMatcher {
    rules: Vec<CompiledRule>,
}

impl RuleMatcher {
    pub fn check(&self, tool_name: &str, input: &serde_json::Value) -> Option<PermissionDecision> {
        let input_str = input.to_string();
        
        for rule in &self.rules {
            // Match tool name (exact or wildcard)
            if rule.tool != tool_name && rule.tool != "*" {
                continue;
            }
            
            // If rule has input pattern, check it
            if let Some(ref re) = rule.input_regex {
                if !re.is_match(&input_str) {
                    continue;
                }
            }
            
            // Rule matched — return decision
            return Some(match rule.action {
                RuleAction::Allow => PermissionDecision::Allow,
                RuleAction::Deny => PermissionDecision::Deny(format!("Denied by rule: {}", tool_name)),
                RuleAction::Ask => PermissionDecision::Ask(format!("Rule requires approval for {}", tool_name)),
            });
        }
        
        None  // No rule matched
    }
}
```

(source: `crates/oxicode-permissions/src/rules.rs:51-106`)

---

## Denial Tracker

### Purpose
Records permission denials for auditing and analytics.

### Structure

```rust
pub struct DenialTracker {
    entries: Mutex<Vec<DenialEntry>>,
}

impl DenialTracker {
    pub fn record(&self, tool_name: &str, reason: &str);
    pub fn history(&self) -> Vec<(String, String, DateTime<Utc>)>;
    pub fn count(&self) -> usize;
}
```

### Usage in Pipeline

```rust
pub fn record_denial(&self, tool_name: &str, reason: &str) {
    self.tracker.record(tool_name, reason);
}

pub fn denial_history(&self) -> Vec<(String, String, DateTime<Utc>)> {
    self.tracker.history()
}
```

Tracks **when** and **why** tools were denied for user review and system monitoring.

(source: `crates/oxicode-permissions/src/tracker.rs`)

---

## TUI Integration

### Permission Dialog Flow

```
[Tool tries to execute]
         ↓
    Pipeline returns Ask
         ↓
CoreEvent::PermissionAsk emitted
    {
        tool_name,
        input_summary,
        prompt,
        reply_tx: oneshot::Sender<PermissionResponse>,
    }
         ↓
TUI renders PermissionDialog widget
    Shows:
    - Tool name & description
    - Input preview
    - Approve / Deny / Allow Always / Deny Always buttons
         ↓
User selects option
         ↓
TUI sends PermissionResponse via reply_tx
    {
        approved: bool,
        remember: RememberLevel,  // Session / Never
    }
         ↓
Tool executes (if approved)
```

### CoreEvent::PermissionAsk

```rust
pub enum CoreEvent {
    /// Permission required — TUI must show dialog and send response via `reply_tx`.
    PermissionAsk {
        tool_name: String,
        input_summary: String,
        prompt: String,
        reply_tx: tokio::sync::oneshot::Sender<oxicode_common::PermissionResponse>,
    },
    // ... other events
}
```

### UiEvent::PermissionResponse

```rust
pub struct PermissionResponse {
    pub approved: bool,
    pub remember: RememberLevel,
}

pub enum RememberLevel {
    Session,  // Add to session_allowlist/denylist
    Never,    // One-time decision
}
```

(source: `crates/oxicode-tui/src/events.rs`)

---

## Session Allow/Deny API

### Adding Session Allowlist

```rust
pipeline.add_session_allow("bash");
```

After user clicks "Always allow" in the permission dialog, the tool is added to `session_allowlist`. All subsequent calls to the same tool skip layers 5-6 (rule matching + default), but **still evaluated by layers 2-3** (hard denies + dangerous patterns).

**Important Security Property:** Session allowlist is AFTER dangerous pattern detection, so:

```rust
// User allowed "bash" once...
pipeline.add_session_allow("bash");

// ...but dangerous command still triggers Ask
let decision = pipeline.check("bash", {"command": "rm -rf /"});
assert!(matches!(decision, PermissionDecision::Ask(_)));
```

### Adding Session Denylist

```rust
pipeline.add_session_deny("bash");
```

After user clicks "Always deny", the tool is added to `session_denylist`. All subsequent calls return `Deny` (except for tools that passed layer 2-3 checks, which still may Ask).

(source: `crates/oxicode-permissions/src/pipeline.rs:145-161`)

---

## AI Classifier (Feature-Gated)

### Purpose
Rule-based (not ML) safety rating for tool invocations. Lightweight heuristic classifier suitable for CLI.

### Enabled With
```bash
cargo build --features ai-classifier
```

### SafetyRating

```rust
pub enum SafetyRating {
    /// High confidence the action is safe.
    Safe,
    /// Moderate confidence — ask user for confirmation.
    Suspicious,
    /// High confidence the action is dangerous.
    Dangerous,
}

pub struct ClassificationResult {
    pub rating: SafetyRating,
    pub confidence: f64,      // 0.0 to 1.0
    pub reason: String,
}
```

### Safe Command Prefixes

```
echo, cat, ls, pwd, whoami, date, uname, head, tail, wc,
sort, uniq, grep, rg, which, env, printenv, id, hostname,
cargo check, cargo test, cargo clippy, cargo fmt,
git status, git log, git diff, git branch,
npm test, npm run lint, yarn test, pnpm test,
rustc --version
```

### Dangerous Patterns

```
rm -rf, rm -fr, mkfs, dd if=, :(){:|:&};:,
chmod 777, chmod -R 777, > /dev/sd, | sh, | bash,
sudo rm, sudo dd, DROP TABLE, DROP DATABASE, TRUNCATE,
--no-verify, force push, git push -f, git reset --hard
```

### Classification Methods

```rust
pub fn classify_command(&self, command: &str) -> ClassificationResult
pub fn classify_file_path(&self, path: &str, is_write: bool) -> ClassificationResult
pub fn classify_tool(&self, tool_name: &str, input: &serde_json::Value) -> ClassificationResult
```

(source: `crates/oxicode-permissions/src/ai_classifier.rs`)

---

## Example: Permission Pipeline In Action

### Scenario 1: Readonly Tool

```rust
let pipeline = PermissionPipeline::new(PermissionMode::Default, vec![]);

let decision = pipeline.check("file_read", ToolPermissionLevel::ReadOnly, 
    &json!({"file_path": "/etc/passwd"}));

// Result: Allow (Layer 1)
assert_eq!(decision, PermissionDecision::Allow);
```

---

### Scenario 2: Dangerous Command in Default Mode

```rust
let decision = pipeline.check("bash", ToolPermissionLevel::ShellExec,
    &json!({"command": "rm -rf /"}));

// Layer 2: Command security checks — PASS
// Layer 3: Dangerous pattern detected "rm -rf /" — ASK
assert!(matches!(decision, PermissionDecision::Ask(_)));
```

---

### Scenario 3: Dangerous Command in Bypass Mode

```rust
let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);

let decision = pipeline.check("bash", ToolPermissionLevel::ShellExec,
    &json!({"command": "rm -rf /"}));

// Layer 2: Command security checks — PASS
// Layer 3: Dangerous detected, but Bypass mode — ALLOW + warn
assert_eq!(decision, PermissionDecision::Allow);
// (warning logged to tracing)
```

---

### Scenario 4: Session Allowlist

```rust
let pipeline = PermissionPipeline::new(PermissionMode::Default, vec![]);

// First call: Ask
assert!(matches!(pipeline.check("bash", ToolPermissionLevel::ShellExec,
    &json!({"command": "cargo test"})), PermissionDecision::Ask(_)));

// User clicks "Always allow" → add to session
pipeline.add_session_allow("bash");

// Next call: Allow (Layer 4 match)
assert_eq!(pipeline.check("bash", ToolPermissionLevel::ShellExec,
    &json!({"command": "cargo test"})), PermissionDecision::Allow);

// But dangerous command still Asks (Layer 3 check)
assert!(matches!(pipeline.check("bash", ToolPermissionLevel::ShellExec,
    &json!({"command": "rm -rf /"})), PermissionDecision::Ask(_)));
```

---

### Scenario 5: User Rule Override

```rust
let rules = vec![
    PermissionRule::allow("bash", Some("cargo.*")),
];
let pipeline = PermissionPipeline::new(PermissionMode::Default, rules);

// Matches rule pattern "cargo.*" → Allow (Layer 6)
assert_eq!(pipeline.check("bash", ToolPermissionLevel::ShellExec,
    &json!({"command": "cargo test"})), PermissionDecision::Allow);

// Non-matching input → Ask (Layer 6 default)
assert!(matches!(pipeline.check("bash", ToolPermissionLevel::ShellExec,
    &json!({"command": "ls -la"})), PermissionDecision::Ask(_)));
```

---

## Testing

The permission pipeline includes comprehensive unit tests:

```bash
cargo test --package oxicode-permissions
```

Key test suites:
- **Readonly fast path** — read-only tools always allowed
- **Bypass mode** — dangerous patterns allowed (with warning), hard denies still enforced
- **Approval-only mode** — non-readonly tools always Ask
- **Session allowlist/denylist** — per-session decisions
- **Dangerous pattern detection** — catches risky patterns
- **Command security** — hard-deny patterns never allowed
- **Rule matching** — user rules work correctly

(source: `crates/oxicode-permissions/src/pipeline.rs:164-367`)

---

## Security Guarantees

1. **Hard Denies NEVER Bypass:** CommandSecurityChecker patterns (LD_PRELOAD, zsh exploits) **always** deny, even in Bypass mode
2. **Dangerous ≠ Hard Deny:** Dangerous patterns (e.g., `rm -rf /`) can be overridden by Bypass mode, but command security cannot
3. **Session Allowlist Respects Security Checks:** A tool allowlisted in session still triggers `Ask` if the input contains dangerous patterns
4. **Layer Order is Enforced:** No layer can be skipped; evaluation always proceeds in order (1→6)

---

## Related Documentation

- **Tool Registry:** `docs/03-tool-system.md`
- **Query Engine:** `docs/02-query-engine.md`
- **System Architecture:** `docs/system-architecture.md`

---

## Unresolved Questions

- Should "Always deny" decisions persist across sessions (stored in config)?
- Should the AI classifier be integrated into the default pipeline (not just feature-gated)?
- What is the UX for viewing/editing session allowlist/denylist during a session?
