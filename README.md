# nexql-mcp

Standalone Postgres MCP server — schema-aware, read-only by default, installable everywhere.

NexQL Pro ships an in-process MCP server locked to VS Code (`pro/src/mcp/`). This repo extracts that capability into an independent Rust binary any MCP client can spawn: Claude Desktop, Cursor, VS Code Copilot, Zed, etc.

**Status:** Phases 0–5 landed + **Phase 6 distribution scaffolding** — `init` client matrix, doctor polish, npm shim + platform stubs, Docker, MCPB manifest, cargo-dist config / release stub. Full cross-platform release CI and musl still TBD.

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

## Local MCP testing

See **[docs/LOCAL.md](docs/LOCAL.md)**. Short version:

```bash
cargo build -p nexql-mcp --release
./scripts/local_mcp_smoke.sh          # throwaway PG + JSON-RPC smoke
# or
npx -y @modelcontextprotocol/inspector ./target/release/nexql-mcp 'postgres://…'
```

Phase 7 (extension cutover) waits until this path is reliable.

### From source

```bash
# pg_query needs clang + libclang
export LIBCLANG_PATH="${LIBCLANG_PATH:-/usr/lib}"   # or your llvm lib dir
cargo build --release -p nexql-mcp
./target/release/nexql-mcp postgres://dev@localhost:5432/appdb
```

### npx (once platform packages are published)

```bash
npx -y nexql-mcp postgres://dev@localhost:5432/appdb
```

Shim: [`npm/bin/nexql-mcp.js`](npm/bin/nexql-mcp.js) resolves `@nexql/mcp-<os>-<arch>` optionalDependencies (stubs under [`npm/packages/`](npm/packages/)).

### Docker

```bash
docker build -t nexql-mcp:0.1.0 .
docker run --rm -i nexql-mcp:0.1.0 postgres://dev@host.docker.internal:5432/appdb
```

### Wire a client

```bash
nexql-mcp init cursor postgres://dev@localhost:5432/appdb
nexql-mcp doctor postgres://dev@localhost:5432/appdb
```

Supported `init` clients: `claude` | `claude-desktop` | `claude-code` | `cursor` | `vscode` | `vscode-copilot` | `zed` | `windsurf` | `continue` | `jetbrains` | `openai-agents`.

Per-client paste blocks: [docs/clients/README.md](docs/clients/README.md).

Config: `~/.config/nexql-mcp/config.toml` (override with `NEXQL_MCP_CONFIG`). See [docs/config.example.toml](docs/config.example.toml).

### Releases (cargo-dist)

[`dist-workspace.toml`](dist-workspace.toml) targets darwin arm64/x64, linux gnu arm64/x64, windows x64. Regenerate CI with `dist generate` (see [`.github/workflows/release.yml`](.github/workflows/release.yml) stub). Musl deferred until pg_query + clang builder setup is validated.

## Development

```bash
cargo check          # workspace compile
cargo run -p nexql-mcp -- doctor
cargo test -p nexql-mcp -- init_clients
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
