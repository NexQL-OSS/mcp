# nexql-mcp

Standalone Postgres MCP server — schema-aware, read-only by default, installable everywhere.

NexQL Pro ships an in-process MCP server locked to VS Code (`pro/src/mcp/`). This repo extracts that capability into an independent Rust binary any MCP client can spawn: Claude Desktop, Cursor, VS Code Copilot, Zed, etc.

**Status:** Phases 0–4 landed — stdio MCP with **24 tools**, resources (`nexql://…`), 7 prompts, ref completions, `index build|status|refresh|clear`, Rust golden parity. Deferred free tools: suggest_indexes / unused indexes / bloat / missing FKs. Next: Phase 5 embeddings or those deferred tools.

## Why this exists

Competing Postgres MCP servers expose `connect → run query → return rows`. Models hallucinate table names against schemas that do not exist. NexQL's moat is the offline schema index (TF-IDF, join graph with inferred FKs, value profiles, optional embeddings, RRF fusion) built in `pro/src/features/dbindex/`. This repo ports that index and the 22 read-only tools from Pro into a fast, trivially installable binary.

## Architecture

```
crates/
├── nexql-mcp/      CLI, subcommands, wiring (binary)
├── nexql-proto/    MCP JSON-RPC types, transports
├── nexql-tools/    tool registry, schemas, executors
├── nexql-index/    dbindex port (builder, store, lexical, joins, embed)
├── nexql-conn/     connection resolution, pool, credentials
└── nexql-policy/   access modes, allow/deny, PII, caps, audit
npm/                npx shim (per-platform optionalDependencies)
mcpb/               one-click Claude Desktop bundle
docs/               per-client setup, tool reference
```

Layering is one-directional: `policy` + `conn` are leaves → `index` → `tools` → binary. `nexql-tools` never depends on `nexql-proto`.

## Quick start (once implemented)

```bash
cargo build --release
./target/release/nexql-mcp postgres://dev@localhost:5432/appdb

# or
npx -y nexql-mcp postgres://dev@localhost:5432/appdb
```

Config: `~/.config/nexql-mcp/config.toml` (override with `NEXQL_MCP_CONFIG`).

See [docs/config.example.toml](docs/config.example.toml).

## Development

```bash
cargo check          # workspace compile
cargo run -p nexql-mcp -- doctor
cargo fmt --all
cargo clippy --workspace --all-targets
```

Read [CLAUDE.md](CLAUDE.md) and [docs/REFERENCE.md](docs/REFERENCE.md) before implementing.

## License

Apache-2.0 for all crates in this repo. Premium extensions (provider embeddings, team sync, hosted gateway) will live in a separate proprietary crate later.

## Roadmap

| Phase | Deliverable |
|-------|-------------|
| 0 | Spike: tokio-postgres + candle MiniLM proof |
| 1 | `nexql-conn` + `nexql-policy` + pg_query validator |
| 2 | MCP stdio transport + ~8 catalog tools |
| 3 | `nexql-index` (byte-compatible with TS format) |
| 4 | Full tool surface, resources, prompts, completions |
| 5 | Local embeddings + RRF fusion |
| 6 | v1.0 ship: cargo-dist, npm, brew, Docker, MCPB |
| 7 | Extension cutover — VS Code spawns binary as MCP client |

Full plan: internal design doc (federated-greeting-badger).

## Reference implementation

TypeScript sources in the sibling `nexql-pro` checkout:

- `pro/src/mcp/NexqlMcpServer.ts`
- `pro/src/mcp/McpResourceProvider.ts`
- `pro/src/mcp/McpPrompts.ts`
- `pro/src/providers/chat/tools/ToolSpec.ts`
- `pro/src/providers/chat/tools/ToolExecutor.ts`
- `pro/src/features/dbindex/*`
