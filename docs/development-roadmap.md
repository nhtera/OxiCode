# Development Roadmap

**Last Updated:** 2026-04-25

## Milestone History

### M1 — Foundation (Complete)
Core workspace, 17-crate architecture, TUI, 49 built-in tools, multi-provider LLM support, permission pipeline, MCP client, session persistence.

### M2 — Parity Baseline (Complete — 2026-04-25)
2468/2468 unit tests green. Full agentic loop functional. All parity TODOs resolved:

| Phase | Feature | Status |
|---|---|---|
| 1 | Project memory injection at startup | Done |
| 2 | Tool search fuzzy match (exact → substring → Levenshtein ≤3) | Done |
| 3 | `/list-dirs`, `/remove-dir`, `/add-dir` persistence; `extra_working_dirs` on `ToolContext` | Done |
| 4 | `/clear` silent (no echo line) | Done |
| 5 | `/status` shows registered command count; TUI status bar `<N> cmds` chip | Done |
| 6 | OTLP telemetry exporter (`telemetry-otlp` feature flag) | Done |
| 7 | Bridge mode: headless WebSocket server, JWT auth, multi-session pool | Done |
| 8 | Anthropic auth header auto-detect (`sk-ant-*` → `x-api-key`, else `Bearer`) | Done |

## Active Work

None — workspace is clean post-M2. See `plans/` for any in-progress plans.

## Upcoming (Candidate)

| Item | Notes |
|---|---|
| Bridge TLS | `OXICODE_BRIDGE_TLS_CERT/KEY` wired but not implemented; deferred to follow-up |
| Extended memory formats | Currently `.md` only; YAML/JSON frontmatter support considered |
| RS256 JWT support | `jsonwebtoken` dep already supports it; awaiting key-management design |
| Provider fallback routing | Weighted fallback across providers on error |

## Deferred / YAGNI

- `dream`, `remote`, `teammate` feature flags — scaffolded, not implemented.
- `voice` feature — Whisper transcription stub.

---

→ See [project-changelog.md](./project-changelog.md) for detailed change history.
