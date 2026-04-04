# OxiCode Documentation Index

**Version:** 0.2.0 | **Last Updated:** 2026-04-04 | **Phase:** 6 Complete

Welcome to OxiCode documentation. This is your guide to understanding, developing, and extending the system.

---

## Quick Navigation

### For New Developers
1. **Start here:** [Project Overview & PDR](./project-overview-pdr.md) — What is OxiCode? What are we building?
2. **Then read:** [System Architecture](./system-architecture.md) — How does it work? What are the layers?
3. **Before coding:** [Code Standards](./code-standards.md) — How do we write code? What are the rules?
4. **Reference:** [Codebase Summary](./codebase-summary.md) — What's in each crate? File organization?

### For Architects
1. [System Architecture](./system-architecture.md) — Data flow, integration points, performance
2. [Codebase Summary](./codebase-summary.md) — Dependency graph, design patterns
3. Phase reports in `../plans/reports/` — Recent decisions, issues found

### For Code Reviewers
1. [Code Standards](./code-standards.md) — What to check in PRs
2. [Codebase Summary](./codebase-summary.md) — Known issues, test coverage
3. CI checks: `cargo clippy`, `cargo fmt`, `cargo test`

### For Contributors
1. [Code Standards](./code-standards.md) — Style, patterns, error handling
2. [System Architecture](./system-architecture.md) — Where does my change fit?
3. Create a GitHub issue first; discuss approach before coding

---

## Documentation Structure

```
docs/
├── index.md                          # This file — navigation hub
├── project-overview-pdr.md           # Vision, phases, features, PDRs
├── system-architecture.md            # Data flow, layers, integration
├── code-standards.md                 # Style, patterns, conventions
└── codebase-summary.md               # Crates, files, design patterns
```

---

## What's New in Phase 6

**Added 4 new TUI widgets, vim text objects, 16 new slash commands:**

### 1. Vim Text Objects
Advanced text object support with operators:
- **Objects** — iw/aw (word), i"/a" (quotes), i(/a( (parens), i{/a{ (braces)
- **Operators** — diw (delete inner word), ci" (change inner quote), ya{ (yank around brace)
- **Composition** — Count prefixes: 3diw, 2ci"
- **Integration** — Works in Normal mode, VisualLine mode (V)

**See:** [System Architecture → Vim Text Objects](./system-architecture.md#vim-text-objects)

### 2. Advanced TUI Dialogs
4 new modal dialog widgets:
- **ContextVisualization** — Visual token budget display, defense layer indicators
- **CostDialog** — Model cost estimation (per-token pricing)
- **AutoModeDialog** — Auto-completion mode selector
- **OAuthDialog** — OAuth 2.0 flow UI for service auth

**See:** [System Architecture → Advanced TUI Dialogs](./system-architecture.md#advanced-tui-dialogs-phase-6-new)

### 3. Extended Commands
16 new slash commands for power users:
- **UI Control** — /color, /statusline, /keybindings, /terminal-setup
- **Content Org** — /tag, /btw (notes), /thinkback (review thinking)
- **Info** — /release-notes, /advisor, /insights, /stickers
- **System** — /passes (optimizations), /rate-limit-options

**See:** [System Architecture → CLI Commands](./system-architecture.md#oxicode-cli-400-loc)

### 4. Test Coverage Expansion
- **TUI Tests:** 81 passing (was ~50 in Phase 5)
- **Text Objects:** Full operator composition tests
- **Dialogs:** Modal interaction tests
- **Zero warnings:** Clippy approved

**See:** [Code Standards → Testing Standards](./code-standards.md)

---

## What's New in Phase 5

**Added Plugin Marketplace & Enterprise Settings:**

### 1. Plugin Registry (oxicode-plugins crate)
Plugin discovery, installation, trust assessment:
- Fetch remote index, cache locally (1-hour TTL)
- Search/filter by keywords, version constraints
- Trust levels: Verified (signed), Community (voted), Unverified
- Hot-reload capability without restart

### 2. Enterprise Managed Settings
Remote admin endpoint with HMAC-SHA256 validation:
- Fetch settings from admin server
- Cloud sync (push/pull with OAuth tokens)
- Conflict resolution (latest-wins)
- Locked keys prevent user override

**See:** [Codebase Summary → Phase 5 Plugin Marketplace](./codebase-summary.md#layer-5-phase-5--plugin-marketplace--enterprise-settings)

---

## Key Architectural Concepts

### 1. Layered Architecture
```
TUI (ratatui)
  ↓
Core Engine (QueryEngine)
  ↓
Tools (Registry, Permission Pipeline)
  ↓
Multi-Provider LLM Interface
  ↓
External Services (Anthropic, OpenAI, MCP)
```

**Plus Phase 4 layers:** Context Defense, Multi-Agent, Skills, Background Tasks

### 2. State Management
Central `StateStore` with watch channel subscribers:
- Single source of truth for app state
- Efficient notifications to UI
- Async-safe mutations

### 3. Error Handling
`OxiResult<T>` everywhere:
- No panics in production code
- Graceful degradation
- User-facing error messages

### 4. Provider Abstraction
All LLM providers implement `LlmProvider` trait:
- Easy swapping (Anthropic, OpenAI, compatible, MCP)
- Streaming support
- Provider-specific features (thinking, caching)

### 5. Tool System
All tools implement `Tool` trait:
- Schema-based validation
- Permission checks via 6-layer pipeline
- Uniform execution interface

---

## Development Workflow

### Before You Code

1. **Check** if an issue exists (GitHub Issues)
2. **Discuss** your approach (comment on issue or create new)
3. **Get approval** from maintainers
4. **Create a branch** from `main`

### While Coding

1. **Follow** [Code Standards](./code-standards.md)
2. **Run tests** frequently: `cargo test --all`
3. **Check linting:** `cargo clippy --all -- -D warnings`
4. **Format code:** `cargo fmt --all`
5. **Write doc comments** for public APIs
6. **Test edge cases** (empty, zero, missing)

### Before Submitting PR

1. **All tests pass:** `cargo test --all`
2. **No clippy warnings:** `cargo clippy --all -- -D warnings`
3. **Code formatted:** `cargo fmt --check`
4. **Commit message** follows conventional format:
   ```
   feat: add context defense layer 4
   fix: divide by zero in BudgetManager
   docs: update system architecture
   ```

### After Review

1. **Address feedback** in new commits (don't amend)
2. **Re-run tests** after changes
3. **Wait for approval** before merging
4. **Merge to main** when approved

---

## Common Tasks

### Adding a New Tool
1. Create `crates/oxicode-tools/src/tools/my_tool.rs`
2. Implement `Tool` trait (name, description, schema, execute)
3. Register in `crates/oxicode-tools/src/registry.rs`
4. Add tests with edge cases
5. Update docs with new tool description

**Reference:** [Code Standards → Tool System](./code-standards.md#trait-implementation)

### Adding a New Command
1. Create `crates/oxicode-cli/src/commands/my_command.rs`
2. Implement `SlashCommand` trait (execute, completions)
3. Register in `crates/oxicode-cli/src/commands/mod.rs`
4. Add tests and help text
5. Update docs with command examples

**Reference:** [System Architecture → CLI Commands](./system-architecture.md#6-cli-commands)

### Adding a New Provider
1. Create `crates/oxicode-api/src/{provider_name}.rs`
2. Implement `LlmProvider` async trait
3. Handle streaming, error cases, provider-specific features
4. Register in `crates/oxicode-api/src/lib.rs`
5. Test with real API calls (use test keys)

**Reference:** [Codebase Summary → oxicode-api](./codebase-summary.md#oxicode-api-600-loc)

### Extending Context Defense
1. Create new layer module in `crates/oxicode-context/src/`
2. Implement defense logic (returns `OxiResult<()>`)
3. Add to `BudgetManager` orchestration
4. Write tests for edge cases (empty, zero budget, etc)
5. Update trigger thresholds in docs

**Reference:** [System Architecture → Context Defense Layers](./system-architecture.md#phase-4-context-defense-layers)

---

## Testing Guide

### Unit Tests
Located in each module's `#[cfg(test)]` block:
```bash
cargo test --lib                      # Run all unit tests
cargo test --lib oxicode_context      # Run context crate tests
cargo test budget_manager              # Run specific test module
```

### Integration Tests
Located in `tests/` directory:
```bash
cargo test --test integration_test
```

### Test Organization
- **Happy path:** Normal operation
- **Edge cases:** Empty input, zero values, missing files
- **Error cases:** Invalid input, permission denied, API errors

**Reference:** [Code Standards → Testing Standards](./code-standards.md#5-testing-standards)

---

## Debugging Tips

### Enable Debug Logging
```bash
RUST_LOG=debug cargo run
RUST_LOG=oxicode_core=trace cargo run  # Specific crate
```

### Run with Clippy
```bash
cargo clippy --all -- -D warnings
```

### Check for Panics
```bash
cargo test --all -- --nocapture 2>&1 | grep panic
```

### Format Check
```bash
cargo fmt --check
```

### Linting
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

---

## Known Issues

### High Priority (Phase 4 Review)
1. **H1:** BudgetManager division by zero when `model_max_tokens == 0`
   - **Fix:** Add guard at top of `check_budget()`
   - **File:** `crates/oxicode-context/src/budget.rs:56`

2. **H2:** TaskRunner `select!` loop breaks on first stream close
   - **Fix:** Track both stdout/stderr EOF separately
   - **File:** `crates/oxicode-tasks/src/runner.rs:73-113`

### Medium Priority (Defensive Hardening)
1. **M1:** spawn_agent_handle doesn't write config to stdin
2. **M2:** Task ID path traversal validation missing
3. **M3:** XML injection in notifications (needs escaping)
4. **M4:** TokenCounter over-counts multi-byte UTF-8

**See:** [Codebase Summary → Known Issues](./codebase-summary.md#known-issues--technical-debt)

---

## Performance Targets

| Operation | Target | Status |
|-----------|--------|--------|
| LLM response | <10s | ✓ Provider latency |
| Tool execution | <500ms | ✓ Most tools fast |
| Permission check | <1ms | ✓ Local pipeline |
| L1 truncation | <10ms | ✓ O(n) where n=messages |
| L2 microcompact | <50ms | ✓ O(n*m) where m=content |
| UI frame render | <100ms | ✓ ratatui efficient |
| Agent spawn | <200ms | ✓ Process creation |

---

## Security Checklist

Before submitting code:

- [ ] No hardcoded secrets (API keys, passwords)
- [ ] No shell injection vulnerabilities
- [ ] No path traversal issues (validate file paths)
- [ ] No panics on untrusted input
- [ ] Error messages don't leak sensitive info
- [ ] Permissions checked before tool execution
- [ ] Input validated against schema

**Reference:** [Code Standards → Error Handling](./code-standards.md#2-error-type-usage)

---

## FAQ

### Q: How do I run the TUI locally?
```bash
cd /path/to/oxicode
cargo run --bin oxicode
```

### Q: Where is the config file?
Global: `~/.oxicode/config.toml`  
Project: `./.oxicode/config.toml` (overrides global)

### Q: How do I add a new skill?
Create `~/.oxicode/skills/my-skill/SKILL.md` with YAML frontmatter + markdown prompt.

### Q: Can I use a different LLM provider?
Yes! Set `provider = "openai"` in config. Supports: anthropic, openai, compatible, mcp.

### Q: What's the token budget?
Configurable per model. Default: 80% of model's max_tokens triggers L3 auto-compact.

### Q: How do I contribute?
1. Check [GitHub Issues](https://github.com/nhtera/oxicode/issues)
2. Discuss your approach
3. Follow [Code Standards](./code-standards.md)
4. Submit PR with tests

---

## Resources

### Internal Documentation
- [Project Overview & PDR](./project-overview-pdr.md) — Vision + requirements
- [System Architecture](./system-architecture.md) — Design + data flow
- [Code Standards](./code-standards.md) — Patterns + conventions
- [Codebase Summary](./codebase-summary.md) — Crates + structure

### Phase Reports
Located in `../plans/reports/`:
- `Explore-260402-oxicode-phase4-interfaces.md` — Architecture exploration
- `code-reviewer-260402-0830-phase4-quality.md` — Quality review + issues
- `tester-260402-0813-phase4-test-results.md` — Test coverage
- `fullstack-developer-260402-0824-integration-wire-crates.md` — Integration notes

### External References
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial)
- [Serde Documentation](https://serde.rs/)
- [Tracing Documentation](https://docs.rs/tracing/)
- [Ratatui Documentation](https://docs.rs/ratatui/)
- [Anthropic API](https://docs.anthropic.com/)
- [OpenAI API](https://platform.openai.com/docs/)

---

## Contact & Support

**Repository:** https://github.com/nhtera/oxicode  
**Issues:** https://github.com/nhtera/oxicode/issues  
**Discussions:** https://github.com/nhtera/oxicode/discussions

---

## Changelog

### Phase 6 (2026-04-04) ✓ Complete
- Added 4 new TUI widgets (dialogs)
- Added vim text objects (iw/aw/i"/a"/i(/a(/i{/a{)
- Added 16 new slash commands
- Extended command total to 91 (from 75)
- Extended tool total to 46 (from 42)
- TUI test coverage: 81 tests (from ~50)

### Phase 5 (2026-04-03) ✓ Complete
- Added oxicode-plugins (2.1K LOC)
- Added enterprise settings to oxicode-config
- Plugin registry + marketplace integration
- Hot-reload capability
- Cloud sync + HMAC validation

### Phase 4 (2026-04-02) ✓ Complete
- Added oxicode-context (5-layer context defense)
- Added oxicode-agents (multi-agent system)
- Added oxicode-skills (skill discovery + activation)
- Added oxicode-tasks (background task management)
- Integrated context defense into QueryEngine
- Code quality review: 0 critical, 2 high-priority issues

### Phase 3 (2026-03-15) ✓ Complete
- Multi-provider support (Anthropic, OpenAI, compatible)
- Permission pipeline (6-layer access control)
- TUI frontend (ratatui, streaming)
- Tool system (33 built-in tools)

### Phase 2 (2026-02-01) ✓ Complete
- Session persistence
- Undo/redo system
- Hooks system

### Phase 1 (2026-01-01) ✓ Complete
- Core LLM integration
- Tool calling support
- Streaming support

### Phase 5 (Planned)
- User-facing agent/skill commands
- Team UI panels
- Skill marketplace
- Advanced compaction strategies

---

**Last Updated:** 2026-04-02  
**Maintained By:** OxiCode Team  
**License:** MIT
