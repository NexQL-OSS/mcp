# Tool reference

Active read-only catalog (24 tools — Phase 2–4):

- **Schema:** `search_schema`, `describe_object`, `get_join_path`, `sample_values`, `get_ddl`, `list_schemas`, `list_objects`, `list_databases`, `list_extensions`
- **Query:** `run_select`, `explain_query`, `explain_analyze`, `analyze_query_plan`
- **Context:** `list_connections`, `get_current_context`, `switch_connection`, `get_index_status`, `server_settings`
- **Perf:** `table_stats`, `index_usage`, `list_running_queries`, `find_blocking_locks`, `slow_queries`, `db_health_check`

Deferred (TODO): `suggest_indexes`, `find_unused_indexes`, `bloat_report`, `find_missing_fks`.

Schemas ported from `ToolSpec.ts` (Phase 4).
