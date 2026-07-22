# nexql-mcp Phase Reference

Condensed from `~/.claude/plans/federated-greeting-badger.md`. Use with [testing.md](testing.md) for per-component test specs.

## Phase 0 — Spike (~1 wk)

**Crate:** `crates/nexql-spike/` or `examples/spike/` (not shipped)

| Build | Test |
|-------|------|
| `tokio-postgres` + `rustls` connect | Integration: `SELECT version()` |
| 3 catalog queries from `catalogQueries.ts` | Integration: seeded `users`/`orders` + FK |
| `candle` MiniLM embed 100 object strings | Unit: dim, no NaN; cosine top-k ranks "user email" → `users.email` |
| Release build metrics | Record cold-start ms, binary size |

**Exit:** Spike README with numbers. Gate behind feature or delete.

---

## Phase 1 — conn + policy (~2.5 wk)

### nexql-conn

Resolution precedence (highest wins):

1. CLI positional URL → 2. `--profile` → 3. flags → 4. `DATABASE_URL`/`POSTGRES_URL` → 5. `PG*` env → 6. `default_profile` → 7. `~/.pgpass` → 8. `--env-file` (opt-in only)

Also: `password_command`, `${env:VAR}`, `deadpool-postgres`, per-checkout `SET default_transaction_read_only` + `statement_timeout`.

**Exit:** ~40 table-driven resolution cases; pool cap integration test.

### nexql-policy

`AccessMode` read/write/admin; schema/table deny globs; PII columns; `max_rows` (500 default); 20k char cap; superuser refusal in write mode.

### pg_query validator

Replace `ToolExecutor.ts:218` prefix check. Allow: `SelectStmt`, `ExplainStmt`, read-only `WITH`. Reject: DML, DDL, stacked statements, CTE-contained DML, comment obfuscation.

**Exit:** `sql_corpus.toml` 100% pass. See [testing.md](testing.md#sql-injection-corpus).

---

## Phase 2 — proto + first tools (~2.5 wk)

### Pre-work (2 days)

Prototype `rmcp` vs hand-rolled. Decide on elicitation/completions/progress coverage.

### nexql-proto (stdio)

`initialize` (verbatim `MCP_SERVER_INSTRUCTIONS`), `ping`, `tools/list`, `tools/call`. Negotiate `2025-06-18` / `2025-03-26` / `2024-11-05`. Unknown method → `-32601`.

### First 8 tools (no index)

`list_connections`, `list_databases`, `list_schemas`, `list_objects`, `get_current_context`, `switch_connection`, `run_select`, `explain_query`.

`run_select` must mirror TS defense: read-only SET (fail-closed), statement_timeout SET, LIMIT wrap `maxRows+1`, truncation flag, 20k char cap.

**Exit:** MCP Inspector stdio smoke. Claude Desktop manual with `init claude` output.

---

## Phase 3 — nexql-index (~5 wk) — SCHEDULE RISK

**Byte-compatible** with TS on-disk format (`formatVersion = 1`).

### Modules

| Module | TS source | Notes |
|--------|-----------|-------|
| `model` | `types.ts` | Serde field names wire-identical |
| `migrate` | `indexFormat.ts` | |
| `catalog` | `catalogQueries.ts` | SQL strings verbatim |
| `lexical` | `lexical.ts` | Tokenizer, TF-IDF, synonyms |
| `joins` | `joinPath.ts` | BFS, max 3 hops |
| `store` | `IndexStore.ts` | Shards, `.lock`, `embeddings.bin` f32 LE |
| `builder` | `IndexBuilder.ts` | **Hardest — budget generously** |
| `query` | `IndexQueryService.ts` | RRF deferred to Phase 5 |

### On-disk files

`manifest.json`, `objects-{schema}-{n}.json`, `tokens.json`, `joingraph.json`, `values.json`, `embeddings.bin`, `embeddings-meta.json`, `overrides.json`, `.lock`.

### Golden-file gate (mandatory)

1. `tests/fixtures/seed_schema.sql` — fixed seed
2. TS `IndexBuilder` → `tests/golden/ts/`
3. Rust builder → byte-compare
4. CI fails on any diff

**Exit:** Golden parity + `search_schema`, `describe_object`, `get_join_path`, `sample_values` E2E.

---

## Phase 4 — full surface (~2.5 wk)

### Remaining tools

`get_ddl`, `table_stats`, `index_usage`, `list_running_queries`, `find_blocking_locks`, `slow_queries`, `db_health_check`, `explain_analyze`, `analyze_query_plan`, `get_index_status` (new).

### New free tools

`suggest_indexes`, `list_extensions`, `server_settings`, `find_unused_indexes`, `bloat_report`, `find_missing_fks`.

### MCP surfaces

Resources (`nexql://` URIs, cursor pagination), prompts (4 + 3 new), completions on `ref` args, `structuredContent`, actionable errors.

**Exit:** All 21 read tools + new tools. Port `McpServer.test.ts` describe blocks. MCP Inspector full checklist.

---

## Phase 5 — embeddings (~2 wk)

`candle` MiniLM local; `embeddings.bin` cross-read with TS; RRF fusion (port `IndexQueryService.test.ts`); context packing; `--embeddings off|local`.

**Exit:** Semantic search beats lexical on synonym fixture.

---

## Phase 6 — v1.0 ship (~2 wk)

`cargo-dist`, npm shim, brew, Docker distroless, MCPB, `doctor`, `init <client>`, MCP Registry.

### Perf budgets (CI-enforced)

| Metric | Budget |
|--------|--------|
| Cold start | <20ms |
| Idle RSS | <25MB |
| `search_schema` p95 warm | <5ms |
| Index build 5000 objects | <30s (excl. embed) |

`cargo-deny` + `cargo-audit` in CI. SBOM per release.

**Exit:** Artifacts installable. Manual client matrix: Claude Desktop, Cursor, VS Code, Zed.

---

## Phase 7 — extension cutover (~2 wk)

Extension spawns `nexql-mcp` binary as MCP client. Delete `pro/src/mcp/*`, `ToolExecutor.ts`, `features/dbindex/*`. Ephemeral 0600 profile files. Pre-cutover index fixture in CI. `check-no-pro.yml` still green.

---

## Phase 8 — HTTP + OAuth (~2.5 wk)

Streamable HTTP from `NexqlMcpServer.ts`: sessions, `Mcp-Session-Id`, LRU 32, idle TTL 30min, bearer auth, 200/min rate limit, 1MB body cap. `--bind 0.0.0.0` requires `--auth`.

---

## Phase 9 — write/admin (~2.5 wk)

`run_write` (elicitation + EXPLAIN preview), `apply_ddl`, `create_index_concurrently`, admin tools. JSONL audit log. Superuser guard.

---

## Phase 10 — pro (ongoing)

Proprietary crate: provider embeddings, team sync, hosted gateway, SSO. Out of Apache-2.0 repo.
