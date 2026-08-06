# nexql-tools

MCP tool registry, JSON schemas, executors, and DBA inspection tools for `nexql-mcp`.

## Overview

Implements all 45 MCP tools across 6 categories:
- **Schema**: `search_schema`, `describe_object`, `get_join_path`, `sample_values`, `get_index_status`, `rebuild_index`, `refresh_index`, `get_schema_diff`, `export_schema_snapshot`, `import_schema_snapshot`, `compare_connections`
- **Query**: `run_select`, `explain_query`, `run_select_aggregate`, `run_select_group`, `full_text_search`, `run_select_json`, `run_select_window`, `copy_to_csv`
- **DBA & Performance**: `auto_tune_query`, `suggest_indexes`, `check_ddl_safety`, `get_table_stats`, `get_slow_queries`, `get_active_queries`, `get_table_bloat`, `get_lock_info`, `get_connection_stats`, `get_cache_hit_ratio`, `get_replication_status`, `get_vacuum_stats`, `get_pg_settings`, `get_extension_info`
- **Write**: `create_table`, `alter_table`, `create_index`, `drop_object`, `run_migration`, `insert_rows`, `update_rows`
- **Connection**: `list_connections`, `switch_connection`, `get_current_context`
- **Diagnostics & Meta**: `run_doctor`, `discover_tools`
