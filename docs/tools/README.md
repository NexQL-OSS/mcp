# Tool reference

Active read-only catalog (32 tools — Phase 2–4b):

- **Schema:** `search_schema`, `describe_object`, `get_join_path`, `sample_values`, `get_ddl`, `list_schemas`, `list_objects`, `list_databases`, `list_extensions`, `find_missing_fks`, `list_roles`
- **Query:** `run_select`, `explain_query`, `explain_analyze`, `analyze_query_plan`, `export_query`, `deep_plan_analysis`
- **Context:** `list_connections`, `get_current_context`, `switch_connection`, `get_index_status`, `server_settings`, `db_dashboard`
- **Perf:** `table_stats`, `index_usage`, `list_running_queries`, `find_blocking_locks`, `slow_queries`, `db_health_check`, `suggest_indexes`, `find_unused_indexes`, `bloat_report`

## Advisory tools (SQL approach)

| Tool | Approach |
|------|----------|
| `suggest_indexes` | High seq-scan tables (`pg_stat_user_tables`) + unindexed FK columns; optional `pg_stat_statements` slow queries; optional `sql` → EXPLAIN plan heuristics |
| `find_unused_indexes` | `pg_stat_user_indexes` where `idx_scan = 0`, excluding PK / UNIQUE / constraint-backed indexes |
| `bloat_report` | Simplified dead-tuple ratio from `pg_stat_user_tables` (`method: dead_tuple_ratio`) — not physical page bloat |
| `find_missing_fks` | Prefer schema-index join-graph `inferred` edges; catalog fallback: `*_id` columns without FK matching a single-column PK |

Schemas ported from `ToolSpec.ts` (Phase 4) plus the four free advisory tools above.
