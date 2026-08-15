export const SITE_VERSION = "0.3.0";

export type ToolCategory =
  | "schema"
  | "query"
  | "context"
  | "performance"
  | "write"
  | "admin"
  | "index"
  | "connection"
  | "meta";

export interface Tool {
  name: string;
  description: string;
  access?: "read" | "write" | "admin";
}

export interface ToolGroup {
  id: ToolCategory;
  title: string;
  summary: string;
  tools: Tool[];
}

export const toolGroups: ToolGroup[] = [
  {
    id: "schema",
    title: "Schema",
    summary: "Discovery, DDL inspection, join paths, and schema comparison.",
    tools: [
      { name: "search_schema", description: "Fuzzy search tables, columns, and comments via the offline index." },
      { name: "inspect_or_search", description: "Combined inspect and search entry point." },
      { name: "search_all_databases", description: "Cross-database schema search." },
      { name: "describe_object", description: "Columns, types, PKs, FKs, and constraints." },
      { name: "get_join_path", description: "Shortest join path between two tables (catalog + inferred FKs)." },
      { name: "sample_values", description: "Distinct values and column statistics from value profiles." },
      { name: "get_ddl", description: "DDL for tables, views, and indexes." },
      { name: "list_schemas", description: "Non-system schemas in the current database." },
      { name: "list_objects", description: "Tables, views, and materialized views in a schema." },
      { name: "list_databases", description: "Databases available on a connection profile." },
      { name: "list_extensions", description: "Installed PostgreSQL extensions." },
      { name: "find_missing_fks", description: "Columns that look like FKs but lack constraints." },
      { name: "list_roles", description: "Database roles and memberships." },
      { name: "schema_diff", description: "Compare schemas between connections." },
      { name: "generate_migration", description: "Migration SQL from a schema diff." },
      { name: "resolve_target", description: "Resolve ambiguous object names." },
      { name: "orient", description: "Orient the agent to current database context." },
    ],
  },
  {
    id: "query",
    title: "Query",
    summary: "Bounded reads, explain plans, exports, and tuning.",
    tools: [
      { name: "run_select", description: "Read-only SELECT/WITH with params, pagination metadata, and row caps." },
      { name: "explain_query", description: "EXPLAIN plan for a query." },
      { name: "export_query", description: "Export query results (CSV, JSON, etc.)." },
      { name: "deep_plan_analysis", description: "Plan hotspots, repeated scans, estimate skew." },
      { name: "auto_tune_query", description: "Plan-based index recommendations for a SQL string." },
    ],
  },
  {
    id: "context",
    title: "Context",
    summary: "Session orientation, connections, and dashboard signals.",
    tools: [
      { name: "list_connections", description: "Configured profiles (never includes passwords)." },
      { name: "get_current_context", description: "Active profile, database, and access mode." },
      { name: "switch_connection", description: "Switch profile or database in the session." },
      { name: "get_index_status", description: "Schema index build state." },
      { name: "server_settings", description: "Key PostgreSQL server parameters." },
      { name: "db_dashboard", description: "Dashboard-style health signals." },
    ],
  },
  {
    id: "performance",
    title: "Performance",
    summary: "Live and historical workload diagnostics.",
    tools: [
      { name: "table_stats", description: "Table-level scan and tuple statistics." },
      { name: "index_usage", description: "Index scan counts and sizes." },
      { name: "list_running_queries", description: "Active queries from pg_stat_activity." },
      { name: "find_blocking_locks", description: "Lock chains and blocking PIDs." },
      { name: "slow_queries", description: "Slow queries via pg_stat_statements." },
      { name: "db_health_check", description: "Quick health snapshot." },
      { name: "suggest_indexes", description: "High seq-scan tables and unindexed FKs." },
      { name: "find_unused_indexes", description: "Indexes with zero scans." },
      { name: "bloat_report", description: "Dead-tuple ratio estimate per table." },
    ],
  },
  {
    id: "write",
    title: "Write",
    summary: "DML and DDL safety — requires write or admin access mode.",
    tools: [
      { name: "execute_sql", description: "DML in write mode; DML+DDL in admin. Supports dry_run.", access: "write" },
      { name: "edit_row", description: "Parameterized insert/update/delete by primary key.", access: "write" },
      { name: "import_data", description: "Batched INSERT from a JSON rows array.", access: "write" },
      { name: "check_ddl_safety", description: "AST-based DDL lock risk analysis via pg_query.", access: "write" },
    ],
  },
  {
    id: "admin",
    title: "Admin",
    summary: "DDL, maintenance, and query termination — admin mode only.",
    tools: [
      { name: "apply_ddl", description: "DDL in a transaction; optional dry_run.", access: "admin" },
      { name: "create_index_concurrently", description: "CREATE INDEX CONCURRENTLY (no transaction).", access: "admin" },
      { name: "run_maintenance", description: "VACUUM, ANALYZE, or REINDEX.", access: "admin" },
      { name: "terminate_query", description: "pg_cancel_backend or pg_terminate_backend by PID.", access: "admin" },
      { name: "rebuild_index", description: "Rebuild the offline schema index.", access: "read" },
      { name: "refresh_index", description: "Incremental index refresh.", access: "read" },
    ],
  },
  {
    id: "connection",
    title: "Connection setup",
    summary: "Profile management without leaking secrets into project config.",
    tools: [
      { name: "setup_connection", description: "Interactive setup via MCP elicitation." },
      { name: "save_profile", description: "Save a connection as a named profile." },
      { name: "test_profile", description: "Test a profile connection." },
      { name: "export_profile", description: "Export profile without secrets." },
      { name: "import_profile", description: "Import profile from export." },
    ],
  },
  {
    id: "meta",
    title: "Meta",
    summary: "Diagnostics and lazy tool discovery.",
    tools: [
      { name: "discover_tools", description: "Lazy tool activation for the meta tool profile." },
      { name: "run_doctor", description: "Connection, permissions, and index diagnostics." },
    ],
  },
];

export const toolCount = toolGroups.reduce((n, g) => n + g.tools.length, 0);
