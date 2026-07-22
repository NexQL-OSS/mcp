# Tool reference

Initial read-only catalog (21 tools — `select_connection_context` dropped):

- **Schema:** `search_schema`, `describe_object`, `get_join_path`, `sample_values`, `get_ddl`, `list_schemas`, `list_objects`, `list_databases`
- **Query:** `run_select`, `explain_query`, `explain_analyze`, `analyze_query_plan`
- **Context:** `list_connections`, `get_current_context`, `switch_connection`
- **Perf:** `table_stats`, `index_usage`, `list_running_queries`, `find_blocking_locks`, `slow_queries`, `db_health_check`

Full schemas port from `ToolSpec.ts` in phase 4.
