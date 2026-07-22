# CLAUDE.md — nexql-mcp

Rust workspace for the standalone NexQL Postgres MCP server. Implementation follows the phased roadmap in `README.md`; crates are scaffolds until their phase lands.

**Agent skill:** `.claude/skills/nexql-mcp-dev/SKILL.md` — read first for session bootstrap, phase discipline, and testing gates.

## Layout

Crate roles and one-directional layering (`policy` + `conn` → `index` → `tools` → binary): see `README.md`. `nexql-tools` must never depend on `nexql-proto`.

## Commands

```bash
cargo check --workspace
cargo run -p nexql-mcp
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

These four are exactly what `.github/workflows/ci.yml` gates on. Clippy warnings fail CI.

## Conventions

- **Edition:** 2024, MSRV pinned in root `Cargo.toml` (`rust-version`)
- **Deps:** add to root `[workspace.dependencies]` first, then `{ workspace = true }` in the member crate. Same for `version`/`edition`/`license`/`repository`.
- **Config:** `~/.config/nexql-mcp/`, env prefix `NEXQL_MCP_*`
- **Data:** `~/.local/share/nexql-mcp/`
- **Resource URIs:** `nexql://<profile>/<database>/…` (unchanged from TS)
- **SQL validation:** `pg_query.rs` — never prefix-string checks
- **Read-only default:** `SET default_transaction_read_only = ON` on every pool connection
- **Index format:** keep compatible with `pro/src/features/dbindex/indexFormat.ts`
- **Licenses:** new deps must satisfy `deny.toml`'s allowlist. Run `cargo deny check` manually — CI does not.

## Git

Feature branches (`feat/…`) → PR to `main`. CI runs on PRs to `main` only.

## Porting map

See `docs/REFERENCE.md` for TS → Rust file mapping.

## IP / licensing

Apache-2.0 here. Pro-only features (provider embeddings, OAuth gateway, audit sinks) stay out of this repo. Do not copy proprietary strings from `nexql-pro` into free artifacts.

## Before phase 2

- Verify `nexql-mcp` is free on crates.io and npm
- Prototype `rmcp` vs hand-rolled `nexql-proto` (2-day spike)
