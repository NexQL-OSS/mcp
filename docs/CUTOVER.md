# Extension cutover (Phase 7)

## Transport decision

**VS Code uses stdio spawn** (`vscode.McpStdioServerDefinition`), not in-process HTTP.

| Option | Verdict |
|--------|---------|
| Finish Rust HTTP sessions + rate-limit to mirror pro | Extra work; only needed for external SSE clients |
| **stdio spawn (chosen)** | Matches Claude Desktop / Cursor / Zed; VS Code MCP host owns the process; no bearer/port UX |

External clients that previously pointed at `http://127.0.0.1:<port>/mcp` should use the standalone `nexql-mcp` binary (`nexql-mcp init <client>`) instead of the extension HTTP endpoint.

## What was deleted vs kept

| Deleted on cutover | Kept (chat still needs it) |
|--------------------|----------------------------|
| `pro/src/mcp/NexqlMcpServer.ts` | `ToolExecutor.ts` |
| `McpResourceProvider.ts` | `features/dbindex/*` |
| `McpPrompts.ts` | Chat / AutoIndex paths |

Cutover removed the in-process MCP HTTP stack and registers a stdio definition that spawns the Rust binary with an ephemeral 0600 profile + `NEXQL_MCP_INDEX_DIR` pointing at the extension `globalStorageUri`.

## Index compatibility

Pre-cutover fixture gate: `crates/nexql-index/tests/pre_cutover_compat.rs` reads
`tests/golden/pre_cutover/` (same layout as `{globalStorage}/dbindex/...`).
Refresh with `scripts/sync_pre_cutover_fixture.sh` after regenerating `expected/`.
