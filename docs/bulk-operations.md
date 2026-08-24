# Bulk operations (large INSERT/UPDATE/DDL)

`execute_sql` and `apply_ddl` run over a pooled connection whose `statement_timeout`
is otherwise fixed once at pool creation (30s default, `nexql-conn/src/pool.rs`).
A bulk load or a heavyweight DDL statement (backfill, large index build outside
`CREATE INDEX CONCURRENTLY`, a big `UPDATE`) can easily exceed that — without a
way to raise it per call, the only lever left is shrinking the batch size by
trial and error, which is what this doc exists to save you from.

## The recipe

1. **Raise the timeout for the call, not the server.** Both `execute_sql` and
   `apply_ddl` accept `timeout_ms`, which — unlike `run_select`'s `timeout_ms`
   (which can only *lower* the timeout below the profile's ceiling) — can
   *raise* it for that one call. It's bounded by `NEXQL_MCP_MAX_TIMEOUT_MS`
   (default 600000ms / 10 minutes), so a single call still can't hold a
   connection open indefinitely.

   ```json
   { "tool": "execute_sql", "args": { "sql": "INSERT INTO events SELECT * FROM staging_events", "timeout_ms": 300000 } }
   ```

2. **Chunk on the agent side, not inside nexql-mcp.** There is no bulk-DML
   chunking helper in this server — deciding how to slice a 15M-row load into
   batches is query-shape and workload-specific, and not something a generic
   SQL tool should own. Loop `execute_sql` calls with `LIMIT`/`OFFSET`,
   keyset pagination, or a staging-table `INSERT ... SELECT ... WHERE id
   BETWEEN $lo AND $hi`, tuning batch size to what fits comfortably inside a
   single `timeout_ms` window instead of the pool's fixed 30s default.

3. **Check the ceiling before you start**, not after a batch times out —
   `db_health_check`'s `statement_timeout_ms` field reports both the active
   profile's default and the `NEXQL_MCP_MAX_TIMEOUT_MS` ceiling, so you know
   the raise budget before picking a batch size.

4. **For DDL, prefer `check_ddl_safety` first.** `apply_ddl` gates CRITICAL-risk
   statements (`DROP TABLE`, `TRUNCATE`, …) behind `confirm_destructive: true`
   — call `check_ddl_safety` to see the classification before deciding whether
   to pass it, or use `dry_run: true` to preview without executing (dry-run
   calls never need `confirm_destructive`).

## Server-side ceiling

| Env var | Default | Applies to |
|---------|---------|------------|
| `NEXQL_MCP_MAX_TIMEOUT_MS` | 600000 (10 min) | Ceiling for `timeout_ms` on `execute_sql`/`apply_ddl` — a per-call *raise*, not a per-profile default |

This is separate from a profile's own `statement_timeout_ms` (`docs/config.example.toml`),
which sets the connection's default ceiling and is what `run_select`'s
`timeout_ms` can only lower below.
