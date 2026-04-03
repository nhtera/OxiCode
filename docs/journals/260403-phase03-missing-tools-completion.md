# Phase 3 Complete: Missing Tools Implementation

**Date**: 2026-04-03 14:00
**Severity**: N/A (feature delivery)
**Component**: Tool Infrastructure, ToolContext
**Status**: Resolved

## What Happened

Completed Phase 3 of the OxiCode gap closure plan by implementing 11 missing tools, closing the tool gap from 31→42 tools (95% coverage of OpenClaude suite). All 82 tests pass, zero clippy warnings.

## The Brutal Truth

This phase felt surgical. After the complexity of Phase 2 (API contracts), implementing individual tools was surprisingly straightforward—each one a contained problem with clear patterns. The hardest part wasn't the code; it was the decision-making around edge cases (LSP notification handling, REPL timeouts, file system safety).

We now have near-complete tool parity with OpenClaude. The gap is closing. This matters because agents can't effectively coordinate work without the right tools—we were forcing workarounds before.

## Technical Details

**Tools Delivered:**
- TodoWriteTool: Session task checklist on ~/.oxicode/todos.json with auto-clear
- TeamCreateTool / TeamDeleteTool: TeamManager delegation for multi-agent workflows
- LspTool: JSON-RPC over stdio, 6 languages auto-detected, 6 LSP operations, 10MB Content-Length cap
- PowerShellTool: Cross-platform pwsh/powershell.exe detection (Windows parity with BashTool)
- ReplTool: Python/Node/Ruby subprocess execution, 600s timeout cap
- McpAuthTool: OAuth token flow and storage for MCP servers
- SuggestBackgroundPrTool: `gh pr create --draft` with 60s timeout + kill_on_drop
- SyntheticOutputTool: Passthrough structured output for SDK sessions
- VerifyPlanExecutionTool: Parses plan markdown checkboxes, reports % completion
- WorkflowTool: Execute .oxicode/workflows/ scripts with path traversal protection

**Critical Issues Fixed During Review:**
- **LSP OOM vector**: Content-Length capped at 10MB to prevent memory exhaustion from malicious/broken servers
- **LSP notification handling**: Added skip loop to discard unsolicited notifications before matching response by `id` field
- **REPL timeout**: Hard cap at 600s prevents infinite loops from freezing the agent
- **TodoWrite test isolation**: Refactored to use ToolContext temp dir instead of user home

## What We Tried

Initial LSP implementation didn't handle notifications—servers send them unprompted. First attempt naively read first response, which could be a notification. Fixed by looping until `id` field matches request.

SyntheticOutput initially considered merging into existing StructuredOutputTool, but semantics differ (validation vs passthrough)—kept separate to avoid entanglement.

## Root Cause Analysis

Phase 3 succeeded because Phase 2 laid foundation: ToolContext infrastructure, TeamManager trait, API contracts. Individual tools were implementable quickly because they had defined boundaries. The real burden was decision-making around safety (timeouts, caps, path protection) and platform differences (Windows vs Unix).

Skipping Tungsten (Anthropic internal) and MCPTool (covered by MCP resource tools) was the right call—95% coverage is sufficient; last 5% are not worth technical debt.

## Lessons Learned

1. **Edge cases compound**: LSP notifications, REPL hangs, file system traversal—each is small, but collectively they represent 40% of implementation effort. Future tool builders need a checklist.

2. **Testing saves hours**: The 82 tests caught notification handling bug immediately. Without them, that would have surfaced in production as cryptic "response mismatch" errors.

3. **Platform parity matters**: PowerShell tool forced us to think about Windows early. Doing this at the end would have been painful—agents can't work reliably if tools fail on half the platforms.

4. **Timeouts are non-negotiable**: 600s REPL cap, 60s SuggestBackgroundPR, 10MB LSP content. These aren't nice-to-haves; they're safety rails preventing runaway processes.

## Next Steps

- **Phase 4**: Built-in Agents + Bundled Skills (implement Agent trait, spawn coordinator/planner/reviewer agents)
- **Phase 5**: Command Implementations (agent CLI entry point, REPL, batch execution)
- **Documentation**: Add tool safety checklist to code standards doc

**Owner**: Implementation lead  
**Timeline**: Phase 4 target completion 2026-04-05
