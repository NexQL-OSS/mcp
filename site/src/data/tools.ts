/**
 * The MCP tool catalog.
 *
 * Tool names and access modes are NOT editorial — they mirror
 * crates/nexql-tools/src/registry.rs. `toolGroups` must contain exactly the
 * entries in `ToolName::ACTIVE` (54 at time of writing), and the `access` field
 * must match what the dispatcher actually gates. Descriptions are ours; the
 * names are the binary's.
 */

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
  /** Omit for read-mode tools. Only write/admin render a badge. */
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
    summary:
      "Finding the right objects and working out how they relate. Most of these read the offline index rather than the live catalog, which is why they are fast enough to call speculatively.",
    tools: [
      {
        name: "search_schema",
        description:
          "Search tables, columns and comments by intent. Returns ranked hits with the columns that matched, so the agent gets four candidates instead of the whole schema.",
      },
      {
        name: "inspect_or_search",
        description:
          "Search and inspect in one call — describes the object when the name is exact, searches when it is not.",
      },
      {
        name: "search_all_databases",
        description:
          "The same search across every database on the profile, for when you are not sure which one holds the table.",
      },
      {
        name: "describe_object",
        description:
          "Columns, types, nullability, primary keys, foreign keys and constraints for one object.",
      },
      {
        name: "get_join_path",
        description:
          "Shortest path between two tables, reporting whether each hop came from a declared foreign key or an inferred edge.",
      },
      {
        name: "sample_values",
        description:
          "What a column actually contains — most-common values, cardinality, null rate — read from the profiles rather than scanning the table.",
      },
      { name: "get_ddl", description: "The CREATE statement for a table, view or index." },
      { name: "list_schemas", description: "Non-system schemas visible in the current database." },
      {
        name: "list_objects",
        description: "Tables, views and materialized views in a schema.",
      },
      {
        name: "list_databases",
        description: "Databases reachable on the current connection profile.",
      },
      { name: "list_extensions", description: "Installed extensions and their versions." },
      {
        name: "find_missing_fks",
        description:
          "Columns that look like foreign keys but have no constraint behind them — usually worth fixing in the schema rather than working around.",
      },
      { name: "list_roles", description: "Database roles and their memberships." },
      {
        name: "schema_diff",
        description: "Compare two schemas, on the same connection or across profiles.",
      },
      {
        name: "generate_migration",
        description: "Turn a schema diff into ordered migration SQL.",
      },
      {
        name: "resolve_target",
        description:
          "Disambiguate a bare object name against the search path and the schema allowlist.",
      },
      {
        name: "orient",
        description:
          "Which profile, database and access mode am I in? Worth calling first — it prevents confident answers about the wrong environment.",
      },
    ],
  },
  {
    id: "query",
    title: "Query",
    summary:
      "Reading data, and understanding why a read is slow. Every statement is parsed with pg_query before it runs, so a mutation hidden in a CTE is caught in read mode.",
    tools: [
      {
        name: "run_select",
        description:
          "Read-only SELECT or WITH, parameterized, row-capped, and returned with pagination metadata so the agent knows there is more.",
      },
      { name: "explain_query", description: "The plan for a statement, without executing it." },
      {
        name: "deep_plan_analysis",
        description:
          "Reads the plan for specific pathologies — repeated scans of one relation, estimates that diverge sharply from actuals — rather than just reporting cost.",
      },
      {
        name: "auto_tune_query",
        description:
          "Proposes indexes from a real plan, targeting the scans that dominate the cost.",
      },
      {
        name: "export_query",
        description: "Write results to CSV or JSON, for extracts too large to pass through context.",
      },
    ],
  },
  {
    id: "context",
    title: "Session & context",
    summary: "Where am I connected, under what mode, and is the index current?",
    tools: [
      {
        name: "list_connections",
        description: "Configured profiles. Never includes passwords, in any mode.",
      },
      {
        name: "get_current_context",
        description: "Active profile, database and access mode.",
      },
      {
        name: "switch_connection",
        description: "Change profile or database mid-session — no restart.",
      },
      {
        name: "get_index_status",
        description: "Whether the schema index exists, how deep it was built, and how stale it is.",
      },
      { name: "server_settings", description: "The server parameters worth knowing about." },
      {
        name: "db_dashboard",
        description: "Connections, sizes, cache hit ratio and activity in one call.",
      },
    ],
  },
  {
    id: "performance",
    title: "Performance & diagnostics",
    summary:
      "Live and historical signals. If something is hanging rather than slow, start with the lock tools — the query is usually fine and simply waiting.",
    tools: [
      { name: "table_stats", description: "Scan counts, tuple counts and sizes per table." },
      {
        name: "index_usage",
        description: "Scan counts and sizes per index — the input to most index decisions.",
      },
      {
        name: "list_running_queries",
        description: "What is executing right now, from pg_stat_activity.",
      },
      {
        name: "find_blocking_locks",
        description: "Resolves the whole lock chain, not just the immediately blocked PID.",
      },
      {
        name: "slow_queries",
        description:
          "Worst offenders from pg_stat_statements, ranked by total time. Needs the extension installed.",
      },
      {
        name: "db_health_check",
        description: "One-call snapshot. Usually makes the next step obvious.",
      },
      {
        name: "suggest_indexes",
        description: "Candidates from high sequential-scan tables and unindexed foreign keys.",
      },
      {
        name: "find_unused_indexes",
        description:
          "Indexes with no scans, already excluding constraint-backed ones, so the list is actionable.",
      },
      { name: "bloat_report", description: "Estimated dead-tuple ratio per table." },
    ],
  },
  {
    id: "index",
    title: "Schema index",
    summary:
      "Maintaining the offline index. Both are read-mode: they write to local index storage, never to your database.",
    tools: [
      {
        name: "rebuild_index",
        description: "Full rebuild. Use after a migration that changed a lot of objects.",
      },
      {
        name: "refresh_index",
        description: "Incremental refresh — cheaper, and enough for most schema changes.",
      },
    ],
  },
  {
    id: "write",
    title: "Write",
    summary:
      "Requires --access-mode write. In read mode these are still listed but return a refusal naming the mode they need, which an agent can report back rather than work around.",
    tools: [
      {
        name: "execute_sql",
        description:
          "DML in write mode, DML and DDL in admin. Supports dry_run, which runs the statement in a transaction and rolls it back.",
        access: "write",
      },
      {
        name: "edit_row",
        description: "Parameterized insert, update or delete addressed by primary key.",
        access: "write",
      },
      {
        name: "import_data",
        description: "Batched inserts from a JSON rows array.",
        access: "write",
      },
      {
        name: "check_ddl_safety",
        description:
          "Parses DDL and reports the lock level it will take — the difference between adding a nullable column and adding one with a volatile default.",
        access: "write",
      },
    ],
  },
  {
    id: "admin",
    title: "Admin",
    summary:
      "Requires --access-mode admin. Against a superuser connection these additionally require --i-know-what-im-doing.",
    tools: [
      {
        name: "apply_ddl",
        description: "DDL inside a transaction, with optional dry_run.",
        access: "admin",
      },
      {
        name: "create_index_concurrently",
        description:
          "CREATE INDEX CONCURRENTLY. Runs outside a transaction because it must, so there is no dry run for it.",
        access: "admin",
      },
      {
        name: "run_maintenance",
        description: "VACUUM, ANALYZE or REINDEX.",
        access: "admin",
      },
      {
        name: "terminate_query",
        description:
          "Cancel or terminate a backend by PID, with guards against killing your own session or a superuser backend.",
        access: "admin",
      },
    ],
  },
  {
    id: "connection",
    title: "Connection setup",
    summary:
      "Creating and testing profiles without putting secrets into project config. Excluded entirely in managed-extension mode.",
    tools: [
      {
        name: "setup_connection",
        description:
          "Guided setup over MCP elicitation — the client prompts for the details, so this works with no terminal.",
      },
      { name: "save_profile", description: "Persist a connection as a named profile." },
      { name: "test_profile", description: "Connect using a profile and report what happened." },
      {
        name: "export_profile",
        description: "Export a profile for sharing. Secrets are omitted by construction.",
      },
      { name: "import_profile", description: "Import a profile from an export." },
    ],
  },
  {
    id: "meta",
    title: "Meta",
    summary: "Diagnostics, and lazy tool activation for context-constrained sessions.",
    tools: [
      {
        name: "discover_tools",
        description:
          "Activates additional tools on demand. This is what makes the meta profile viable: 13 tools up front, the rest available when asked for.",
      },
      {
        name: "run_doctor",
        description:
          "Checks connection, permissions, extensions and index state. Run it first when something is not working — it isolates the problem from any MCP client.",
      },
    ],
  },
];

export const toolCount = toolGroups.reduce((n, g) => n + g.tools.length, 0);
