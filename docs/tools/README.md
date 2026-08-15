Active catalog (54 tools across Schema, Query, Context, Perf, Write, Admin, and Meta):

nexql-mcp is a standalone Postgres MCP server: schema-aware SQL exploration, query
tuning, index/DDL safety, and live database diagnostics for an LLM agent talking to
PostgreSQL over stdio.

- **Schema:** `search_schema`, `inspect_or_search`, `search_all_databases`, `describe_object`, `get_join_path`, `sample_values`, `get_ddl`, `list_schemas`, `list_objects`, `list_databases`, `list_extensions`, `find_missing_fks`, `list_roles`, `schema_diff`, `generate_migration`, `resolve_target`, `orient`
- **Query:** `run_select`, `explain_query`, `export_query`, `deep_plan_analysis`, `auto_tune_query`
- **Context:** `list_connections`, `get_current_context`, `switch_connection`, `get_index_status`, `server_settings`, `db_dashboard`
- **Perf:** `table_stats`, `index_usage`, `list_running_queries`, `find_blocking_locks`, `slow_queries`, `db_health_check`, `suggest_indexes`, `find_unused_indexes`, `bloat_report`
- **Write (Write+):** `execute_sql`, `edit_row`, `import_data`, `check_ddl_safety`
- **Admin:** `apply_ddl`, `create_index_concurrently`, `run_maintenance`, `terminate_query`
- **Index maintenance:** `rebuild_index`, `refresh_index`
- **Connection setup:** `setup_connection`, `save_profile`, `test_profile`, `export_profile`, `import_profile`
- **Meta:** `discover_tools` (lazy tool activation for the `meta` tool profile), `run_doctor`

## Access gating

All tools appear in `tools/list` regardless of `--access-mode`. At call time:

| Mode | Read tools | Write tools | Admin tools |
|------|------------|-------------|-------------|
| `read` (default) | allowed | refused | refused |
| `write` | allowed | allowed | refused |
| `admin` | allowed | allowed | allowed |

Write/admin tools return an error in read mode. Startup also refuses write/admin against a superuser connection unless `--i-know-what-im-doing` is set.

## Write tools

| Tool | Notes |
|------|-------|
| `execute_sql` | DML in write mode; DML+DDL in admin mode. Explicit transaction; `dry_run=true` rolls back; errors roll back |
| `edit_row` | Parameterized insert/update/delete by PK columns |
| `import_data` | Batched INSERT from JSON `rows` array |
| `apply_ddl` | DDL only; transaction + optional `dry_run` |
| `create_index_concurrently` | **No transaction** — `CREATE INDEX CONCURRENTLY` only |
| `run_maintenance` | **No transaction** — `VACUUM` / `ANALYZE` / `REINDEX` |
| `terminate_query` | `pg_cancel_backend` or `pg_terminate_backend` by pid; refuses superuser targets and own session |

## Advisory tools (SQL approach)

| Tool | Approach |
|------|----------|
| `suggest_indexes` | High seq-scan tables (`pg_stat_user_tables`) + unindexed FK columns; optional `pg_stat_statements` slow queries; optional `sql` → EXPLAIN plan heuristics |
| `find_unused_indexes` | `pg_stat_user_indexes` where `idx_scan = 0`, excluding PK / UNIQUE / constraint-backed indexes |
| `bloat_report` | Simplified dead-tuple ratio from `pg_stat_user_tables` (`method: dead_tuple_ratio`) — not physical page bloat |
| `find_missing_fks` | Prefer schema-index join-graph `inferred` edges; catalog fallback: `*_id` columns without FK matching a single-column PK |

Schemas ported from `ToolSpec.ts` (Phase 4) plus the four free advisory tools above.
