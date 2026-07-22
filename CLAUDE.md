# CLAUDE.md — nexql-mcp

Rust workspace for the standalone NexQL Postgres MCP server. **This session scope is initialization only** — see README roadmap for implementation phases.

## Layout

| Path | Role |
|------|------|
| `crates/nexql-mcp/` | Binary: CLI (`clap`), subcommands, wiring |
| `crates/nexql-proto/` | MCP JSON-RPC; no tool logic |
| `crates/nexql-tools/` | Tools return typed results; **must not** depend on `nexql-proto` |
| `crates/nexql-index/` | dbindex port; on-disk format byte-compatible with TS |
| `crates/nexql-conn/` | libpq-style resolution ladder, pool, credentials |
| `crates/nexql-policy/` | access modes, deny lists, caps, audit |
| `npm/` | `npx -y nexql-mcp` shim (esbuild optionalDeps pattern) |
| `mcpb/` | Claude Desktop one-click bundle |
| `docs/` | client setup snippets, tool reference |

## Commands

```bash
cargo check
cargo run -p nexql-mcp
cargo fmt --all
cargo clippy --workspace --all-targets
```

## Conventions

- **Edition:** 2024, MSRV pinned in root `Cargo.toml` (`rust-version`)
- **Config:** `~/.config/nexql-mcp/`, env prefix `NEXQL_MCP_*`
- **Data:** `~/.local/share/nexql-mcp/`
- **Resource URIs:** `nexql://<profile>/<database>/…` (unchanged from TS)
- **SQL validation:** `pg_query.rs` — never prefix-string checks
- **Read-only default:** `SET default_transaction_read_only = ON` on every pool connection
- **Index format:** keep compatible with `pro/src/features/dbindex/indexFormat.ts`

## Porting map

See `docs/REFERENCE.md` for TS → Rust file mapping.

## IP / licensing

Apache-2.0 here. Pro-only features (provider embeddings, OAuth gateway, audit sinks) stay out of this repo. Do not copy proprietary strings from `nexql-pro` into free artifacts.

## Before phase 2

- Verify `nexql-mcp` is free on crates.io and npm
- Prototype `rmcp` vs hand-rolled `nexql-proto` (2-day spike)
