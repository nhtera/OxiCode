# Project Changelog

## 2026-04-25 — Parity Fix (M2 complete)

All 8 remaining parity gaps resolved. `cargo test --workspace` stays green at 2468 tests.

**Features added:**

- **Project memory injection** — Reads `~/.oxicode/projects/{key}/memory/MEMORY.md` plus additional `.md` files at startup; injected into system prompt; falls back to cwd if no git root; capped at 100 KB.
- **Tool search fuzzy match** — `tool_search` now queries the registry: exact name match → substring match → Levenshtein distance ≤ 3.
- **Working dirs persistence** — `/add-dir` persists entries in `Session`; `/list-dirs` and `/remove-dir` added. `ToolContext` exposes `extra_working_dirs`; glob, grep, and file_read consult extras when no explicit path given.
- **Silent `/clear`** — `/clear` no longer echoes a confirmation line.
- **Command count in status** — `/status` output includes `Slash commands: <N> registered`; TUI status bar gains an `<N> cmds` chip.
- **OTLP telemetry** (`telemetry-otlp` feature) — Wires OpenTelemetry OTLP exporter via `opentelemetry@0.27` + `tonic`. Env: `OXICODE_OTLP_ENDPOINT` (default `http://localhost:4317`), `OXICODE_OTLP_HEADERS`, `OXICODE_OTLP_SAMPLE_RATE` (default 0.1).
- **Bridge mode** (`bridge` feature) — `--bridge` flag launches headless WebSocket server with JWT auth (HS256, subprotocol `oxicode.bridge.v1`). Env: `OXICODE_BRIDGE_JWT_SECRET`, `OXICODE_BRIDGE_MAX_SESSIONS` (default 16), `OXICODE_BRIDGE_IDLE_TIMEOUT_SECS` (default 600). TLS env vars defined; TLS implementation deferred.
- **Auth header auto-detect** — `AnthropicProvider::with_token_auto_detect`: `sk-ant-*` tokens use `x-api-key`; all others use `Authorization: Bearer`. Override via `OXICODE_AUTH_HEADER=bearer|x-api-key`.

**Docs added/updated:** `bridge-mode.md` (new), `operations.md` (bridge section added), `00-index.md` (reading order corrected, new doc links), `01-architecture.md` (stale placeholder language removed), `development-roadmap.md` (new), `project-changelog.md` (new).

---

## Earlier

Pre-M2 history not yet backfilled. See git log for details.
