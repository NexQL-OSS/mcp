---
name: nexql-mcp-dev
description: >-
  Guides phased implementation and testing of the standalone Rust nexql-mcp Postgres
  MCP server. Use when working in this repo, implementing nexql-mcp crates, porting
  from pro/src/mcp or dbindex, writing MCP tools, or starting a new session here.
---

# nexql-mcp Development

Standalone Rust MCP server. Ports Pro's in-process MCP + dbindex into an installable binary. Design doc: `~/.claude/plans/federated-greeting-badger.md` (workspace root).

## Session bootstrap (do this first)

1. **Confirm repo and branch**
   ```bash
   git branch --show-current && git status -sb
   ```
   Active implementation branch: `feat/mcp-server-impl`. This directory is its own git repo.

2. **Read project context** (in order, stop when you have enough):
   - `CLAUDE.md` — layout, conventions, hard rules
   - `docs/REFERENCE.md` — TS → Rust porting map
   - [phases.md](phases.md) — current phase deliverables + exit gates
   - [testing.md](testing.md) — what/how to test per logical component

3. **Determine current phase** — check README status, recent commits, and which exit gates are met. **Do not skip phases.** Do not start Phase N+1 until Phase N exit gate passes.

4. **Identify TS reference** before implementing any component — see porting table in `docs/REFERENCE.md`. TS sources live in sibling checkouts: `../pro/`, `../core/`.

## Architecture (non-negotiable)

```
policy + conn (leaves) → index → tools → binary (nexql-mcp)
proto (transport only; no tool logic)
```

| Rule | Detail |
|------|--------|
| `nexql-tools` ↛ `nexql-proto` | Tools return typed results; proto serializes |
| SQL validation | `pg_query.rs` AST walk — **never** prefix-string checks |
| Read-only default | `SET default_transaction_read_only = ON` on every pool checkout |
| Index format | Byte-compatible with `../pro/src/features/dbindex/indexFormat.ts` |
| Resource URIs | `nexql://<profile>/<database>/…` unchanged |
| Config / env | `~/.config/nexql-mcp/`, `NEXQL_MCP_*` |
| License | Apache-2.0 only — no pro strings, OAuth gateway, provider embeddings |

## Implementation workflow

For each logical unit (function, module, tool):

```
1. Read TS reference (if porting)
2. Implement minimal correct Rust in the right crate
3. Add tests BEFORE marking done (see testing.md for type + cases)
4. cargo fmt && cargo clippy --workspace --all-targets -- -D warnings
5. cargo test --workspace (or scoped: cargo test -p nexql-conn)
6. Update docs/TEST_PARITY.md if porting an McpServer.test.ts case
```

**Do not** add dependencies without flagging why. **Do not** commit unless asked.

## Phase discipline

| Phase | Focus | Exit gate (summary) |
|-------|-------|---------------------|
| 0 | Spike: tokio-postgres + candle | Measured cold-start/RSS/embed; throwaway crate |
| 1 | `nexql-conn` + `nexql-policy` + pg_query | Injection corpus 100%; ~40 resolution cases; pool integration |
| 2 | stdio MCP + 8 catalog tools | MCP Inspector smoke; 8 tools integration green |
| 3 | `nexql-index` | **Golden-file byte parity with TS builder** |
| 4 | Full tools, resources, prompts | 21 read tools + parity checklist |
| 5 | Embeddings + RRF | Cross-lang embed read; semantic search fixture |
| 6 | Ship: dist, npm, perf budgets | CI perf budgets; client matrix manual |
| 7 | Extension cutover | Extension spawns binary; pre-cutover index loads |
| 8 | HTTP + OAuth | Port NexqlMcpServer.ts session/auth tests |
| 9 | Write/admin modes | Elicitation + audit log |

Full detail: [phases.md](phases.md).

## Critical gates (never waive)

1. **Phase 3 golden files** — TS `IndexBuilder` output committed under `tests/golden/ts/`; Rust builder must match bytes. Highest-value test in the project.
2. **SQL injection corpus** — `WITH x AS (DELETE …) SELECT`, stacked statements, comment tricks must all reject. TS prefix check is the weakness being fixed.
3. **TS test parity** — `../pro/test/McpServer.test.ts` is the porting checklist (40+ cases). Track in `docs/TEST_PARITY.md`.

## Key reference files (sibling repos)

| Rust target | TS source |
|-------------|-----------|
| `nexql-proto` | `../pro/src/mcp/NexqlMcpServer.ts` |
| `nexql-tools::schema` | `../pro/src/providers/chat/tools/ToolSpec.ts` |
| `nexql-tools::exec` | `../pro/src/providers/chat/tools/ToolExecutor.ts` |
| `nexql-index::*` | `../pro/src/features/dbindex/*` |
| `nexql-tools::sql` | `../core/src/commands/sql/{profile,monitoring}.ts` |
| Resources/prompts | `../pro/src/mcp/McpResourceProvider.ts`, `McpPrompts.ts` |

**Dropped:** `select_connection_context` → MCP `elicitation/create`.

## Commands

```bash
cargo check --workspace
cargo test --workspace
cargo test -p nexql-policy          # scoped
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p nexql-mcp -- doctor    # once implemented
```

## Pre-phase-2 checklist

- [ ] Verify `nexql-mcp` free on crates.io and npm
- [ ] 2-day spike: `rmcp` vs hand-rolled `nexql-proto` (elicitation, completions, progress)

## When stuck

| Problem | Action |
|---------|--------|
| Unsure which crate | Check layering table above; `docs/REFERENCE.md` porting map |
| Behavior mismatch with Pro | Read TS source + `../pro/test/McpServer.test.ts` |
| Index format question | Read `../pro/src/features/dbindex/types.ts` + `indexFormat.ts` |
| Security / SQL | Read `ToolExecutor.ts:216` (what NOT to do) + injection corpus in testing.md |
