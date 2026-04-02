# Changelog

All notable changes to OxiCode will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- 17-crate workspace architecture
- Multi-provider LLM abstraction (Anthropic Claude)
- 49 built-in tools (bash, file_read, file_write, glob, grep, git, etc.)
- 6-layer permission pipeline with safe allowlist, dangerous pattern detection, command security
- AI permission classifier (feature-gated: `ai-classifier`)
- Ratatui terminal UI with markdown rendering, syntax highlighting, themes
- MCP client with 4 transports (stdio, SSE, streamable HTTP, WebSocket)
- Multi-agent system with isolated subagent contexts
- Skill discovery and execution framework
- Session persistence (save/load/resume)
- 105+ slash commands
- NDJSON structured output mode (`--output json`)
- Shell completions (bash, zsh, fish, powershell)
- Man page generation
- 26 lifecycle hook events
- 5-layer context defense system
- Background task management
- Subprocess plugin system
- Cross-platform CI (macOS, Linux, Windows)
- Release pipeline with GitHub Actions
- Install script (`curl | sh`)
- Criterion benchmarks (startup, token counting)
- Release profiles (LTO, strip, codegen-units=1)
- 276+ tests across all crates
