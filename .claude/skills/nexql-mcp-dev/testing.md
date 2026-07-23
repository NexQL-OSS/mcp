# nexql-mcp Testing Guide

Test-first discipline: every logical unit ships with tests matching its phase. Do not mark a phase done until its exit gate passes.

## Test infrastructure

Set up once before Phase 1 implementation lands.

| Layer | Location | Tooling | Introduced |
|-------|----------|---------|------------|
| Unit | `#[cfg(test)]` in each crate | `rstest` or `#[test_case]` | Phase 0+ |
| Golden files | `crates/nexql-index/tests/golden/` | byte-compare or `insta` | Phase 3 |
| Integration | `tests/integration/` workspace crate | `testcontainers`, PG 14–17 matrix | Phase 1+ |
| Protocol | `tests/mcp_conformance/` | MCP Inspector CLI, scripted JSON-RPC | Phase 2+ |
| Security corpus | `crates/nexql-policy/tests/fixtures/sql_corpus.toml` | static, no DB | Phase 1 |
| Perf | `benches/` + CI job | `criterion` or custom timer | Phase 6 |

### Shared fixtures

```
tests/
├── fixtures/
│   └── seed_schema.sql          # fixed schema for golden + integration
├── golden/
│   └── ts/                      # TS IndexBuilder output (committed)
└── integration/
    └── ...                      # testcontainers harness
```

### CI evolution (`.github/workflows/ci.yml`)

| Phase | Add to CI |
|-------|-----------|
| 0 | `cargo test` for spike |
| 1 | `cargo-deny`, `cargo-audit` (wire `deny.toml`) |
| 2 | MCP Inspector stdio smoke |
| 3 | Golden-file diff job (blocks merge) |
| 6 | Perf budgets, cross-platform matrix, SBOM |

---

## SQL injection corpus

File: `crates/nexql-policy/tests/fixtures/sql_corpus.toml`

Each entry: `{ sql, expect, tool_context? }` where `expect` is `allow` or `reject`.

### Must reject (Rust must fix TS gaps)

```toml
[[case]]
sql = "SELECT 1; DROP TABLE t"
expect = "reject"
note = "stacked statements"

[[case]]
sql = "WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x"
expect = "reject"
note = "CTE DML — passes TS prefix check today"

[[case]]
sql = "/* c */ DELETE FROM t"
expect = "reject"

[[case]]
sql = "--\nDELETE FROM t"
expect = "reject"

[[case]]
sql = "EXPLAIN DELETE FROM t"
expect = "reject"
```

### Must allow

```toml
[[case]]
sql = "SELECT 1"
expect = "allow"

[[case]]
sql = "WITH cte AS (SELECT 1) SELECT * FROM cte"
expect = "allow"

[[case]]
sql = "EXPLAIN SELECT 1"
expect = "allow"
```

### Port from TS tests (`../pro/test/McpServer.test.ts`)

| Line | Case | Expected |
|------|------|----------|
| ~483 | `SET default_transaction_read_only` fails | No SELECT executed |
| ~500 | `SET statement_timeout` fails | No SELECT executed |
| ~518 | LIMIT wrap applied | Outer LIMIT maxRows+1 |
| ~539 | rows > maxRows | `truncated: true` |
| ~827 | `explain_analyze` on DELETE | Security error |
| ~883 | ref `public.users; DROP TABLE x` | Invalid object reference |

---

## Per-phase test matrix

### Phase 0

| Component | Type | Assert |
|-----------|------|--------|
| PG connect | integration | version() returns |
| Catalog queries | integration | ≥1 table, column, FK |
| Embed 100 objs | unit | dim consistent, no NaN |
| Cosine search | unit | "user email" → users.email top-3 |

### Phase 1 — nexql-conn

Table-driven `resolve.rs` tests (~40 cases):

| Case family | Assert |
|-------------|--------|
| CLI arg + DATABASE_URL | CLI wins |
| Profile + flags | Profile wins over flags |
| PGHOST/PGUSER/PGDATABASE | Composed correctly |
| default_profile in config | Used when nothing else set |
| ~/.pgpass | Password matched by host/port/db/user |
| .env in cwd without --env-file | Ignored |
| --env-file | Vars loaded |
| password_command failure | Error surfaced, no connect |

Pool integration: N concurrent queries respect `max_connections`. After checkout: `SHOW default_transaction_read_only` = `on`.

### Phase 1 — nexql-policy

| Function | Cases |
|----------|-------|
| Schema allow/deny | allowed refs filtered |
| Table glob `auth.*` | blocks `auth.sessions` |
| PII columns | excluded from policy decisions |
| max_rows | default 500, clamp 1–10000 |
| result cap | truncation at 20000 chars |
| superuser + write mode | startup error without override |

### Phase 2 — proto

| Method | Assert |
|--------|--------|
| initialize | instructions byte-match `MCP_SERVER_INSTRUCTIONS` |
| unknown protocol version | negotiates to newest supported |
| ping | empty result |
| unknown method | -32601 |
| tools/list | 8 tools present |

### Phase 2 — first 8 tools

Port `../pro/test/McpServer.test.ts` cases by line reference. Each tool: integration against testcontainers seeded DB.

`run_select` defense-in-depth checklist:

- [ ] read_only SET fails → abort, no query
- [ ] statement_timeout SET fails → abort
- [ ] LIMIT wrap present
- [ ] truncated flag when over maxRows
- [ ] result ≤ 20k chars

### Phase 3 — index (golden gate)

**Procedure:**

```bash
# 1. Start PG, apply seed
psql -f tests/fixtures/seed_schema.sql

# 2. Generate TS golden (one-time or on schema change)
# (run from ../pro checkout with IndexBuilder)

# 3. Rust builder
cargo test -p nexql-index --test golden_parity

# 4. CI compares bytes of:
#    manifest.json, objects-*.json, tokens.json, joingraph.json
```

| Module | Test type | Assert |
|--------|-----------|--------|
| model | unit | serde round-trip vs fixture JSON |
| catalog | unit | SQL strings == TS constants |
| lexical | golden | token lists, TF-IDF scores (epsilon) |
| joins | unit | BFS paths, max 3 hops, no-path message |
| store | unit + golden | shard limits, embeddings.bin layout, cross-read |
| builder | **golden** | byte-identical to TS output |
| query | integration | search_schema ranks correctly |

### Phase 4 — full surface

Track parity in `docs/TEST_PARITY.md`:

| TS describe block | Phase | Cases |
|-------------------|-------|-------|
| Security (401, 405, 400, 413, rate limit) | 8 | defer HTTP |
| Protocol & Tool Dispatching | 2–4 | ~15 cases |
| Spec Hardening | 2, 4, 8 | structuredContent, capabilities |
| Resources & Prompts | 4 | 8 cases |
| Monitoring Tools | 4 | 6 cases |

Resources: `nexql://` URIs, cursor -32602, unknown -32002.
Prompts: list/get, missing arg rejected.
Errors: actionable — `"Object X not found — call search_schema(...)"`.

### Phase 5 — embeddings

| Test | Assert |
|------|--------|
| MiniLM vector | epsilon match vs TS for fixed string |
| embeddings.bin | TS IndexStore reads Rust output |
| fuseRrf | port IndexQueryService.test.ts |
| --embeddings off | lexical only |

### Phase 6 — ship

| Test | How |
|------|-----|
| Cold start <20ms | `/usr/bin/time` on `--version` |
| RSS <25MB | smaps after idle |
| search_schema p95 <5ms | criterion on golden index |
| npm shim | CI install on ubuntu + macos |
| Docker | `docker run` stdio smoke |
| cargo-deny/audit | CI clean |

### Phase 7–9

| Phase | Key tests |
|-------|-----------|
| 7 | Extension E2E spawn; pre-cutover index loads; check-no-pro.yml |
| 8 | Session isolation, auth, rate limit — full McpServer.test.ts security block |
| 9 | Elicitation rollback; audit JSONL parse; admin-only tools |

---

## Running tests

```bash
cargo test --workspace
cargo test -p nexql-conn
cargo test -p nexql-policy
cargo test -p nexql-index --test golden_parity
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Integration tests need Docker for testcontainers.

---

## Definition of done (per unit)

- [ ] Tests added matching this guide for the logical component
- [ ] TS parity case tracked in `docs/TEST_PARITY.md` if applicable
- [ ] No prefix-string SQL validation
- [ ] `cargo clippy` clean
- [ ] Phase exit gate still met (no regressions)
