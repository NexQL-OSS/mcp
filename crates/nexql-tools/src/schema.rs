// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Tool descriptors for the active MCP surface (Phase 2–4).

use serde_json::{Value, json};

use crate::registry::{ToolName, ToolProfile};

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: ToolName,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Tools filtered by the requested `ToolProfile`.
pub fn tools_for_profile(profile: ToolProfile) -> Vec<ToolSpec> {
    let names = ToolName::for_profile(profile);
    active_tools()
        .into_iter()
        .filter(|spec| names.contains(&spec.name))
        .collect()
}

/// Generate a formatted Mermaid ERD snippet for an object's column & key structure.
pub fn generate_mermaid_erd_for_object(obj: &serde_json::Map<String, Value>) -> Option<String> {
    let ref_name = obj.get("ref").and_then(|v| v.as_str()).unwrap_or("table");
    let safe_table_name = ref_name.replace(['.', '-'], "_");
    let mut diagram = String::from("erDiagram\n");
    diagram.push_str(&format!("    {safe_table_name} {{\n"));
    if let Some(columns) = obj.get("columns").and_then(|v| v.as_array()) {
        for col in columns {
            let name = col.get("name").and_then(|v| v.as_str()).unwrap_or("col");
            let data_type = col.get("type").and_then(|v| v.as_str()).unwrap_or("string");
            let pk = col.get("is_pk").and_then(|v| v.as_bool()).unwrap_or(false);
            let fk = col.get("is_fk").and_then(|v| v.as_bool()).unwrap_or(false);
            let key_str = match (pk, fk) {
                (true, true) => " PK,FK",
                (true, false) => " PK",
                (false, true) => " FK",
                _ => "",
            };
            diagram.push_str(&format!(
                "        {} {}{}\n",
                data_type.replace(' ', "_"),
                name,
                key_str
            ));
        }
    }
    diagram.push_str("    }\n");
    Some(diagram)
}

/// Generate a formatted Mermaid ERD diagram snippet for a FK join path.
pub fn generate_mermaid_diagram_for_path(path_val: &Value) -> Option<String> {
    let edges = path_val
        .as_array()
        .or_else(|| path_val.get("path").and_then(|v| v.as_array()))?;
    if edges.is_empty() {
        return None;
    }
    let mut diagram = String::from("erDiagram\n");
    for edge in edges {
        let from = edge
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or("A")
            .replace(['.', '-'], "_");
        let to = edge
            .get("to")
            .and_then(|v| v.as_str())
            .unwrap_or("B")
            .replace(['.', '-'], "_");
        let from_col = edge.get("from_col").and_then(|v| v.as_str()).unwrap_or("");
        let to_col = edge.get("to_col").and_then(|v| v.as_str()).unwrap_or("");
        diagram.push_str(&format!(
            "    {from} }}|--|| {to} : \"{from_col} -> {to_col}\"\n"
        ));
    }
    Some(diagram)
}

/// Phase 2 catalog tools (live Postgres; no index required).
pub fn phase2_catalog_tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: ToolName::ListConnections,
            description: "List configured connection profiles (never includes passwords).",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::ListDatabases,
            description: "List databases available for a connection profile.",
            input_schema: object_schema(&[(
                "connectionId",
                "string",
                true,
                "Name of a configured connection profile, as returned by list_connections.",
            )]),
        },
        ToolSpec {
            name: ToolName::ListSchemas,
            description: "List non-system schemas in the currently selected database.",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::ListObjects,
            description: "List objects (tables, views, …) in a schema.",
            input_schema: object_schema(&[
                (
                    "schema",
                    "string",
                    false,
                    "Schema name to list, e.g. \"public\". Defaults to all non-system schemas.",
                ),
                (
                    "kind",
                    "string",
                    false,
                    "Filter by object kind: \"table\", \"view\", or \"materialized_view\".",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::GetCurrentContext,
            description: "Return the active profile, database, and access mode.",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::SwitchConnection,
            description: "Switch the session to another connection profile / database.",
            input_schema: object_schema(&[
                (
                    "connectionId",
                    "string",
                    true,
                    "Name of a configured connection profile, as returned by list_connections.",
                ),
                (
                    "database",
                    "string",
                    false,
                    "Database name on that connection. Defaults to the profile's configured database.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::RunSelect,
            description: "Run a read-only SELECT or WITH query. DML/DDL are rejected. Only reference tables/columns confirmed via list_schemas / list_objects.",
            input_schema: object_schema(&[(
                "sql",
                "string",
                true,
                "A single SELECT or WITH statement, schema-qualify table names where possible.",
            )]),
        },
        ToolSpec {
            name: ToolName::ExplainQuery,
            description: "Run EXPLAIN (no ANALYZE execute) for a SELECT/WITH query.",
            input_schema: object_schema(&[(
                "sql",
                "string",
                true,
                "A single SELECT or WITH statement to explain (not executed).",
            )]),
        },
        ToolSpec {
            name: ToolName::DiscoverTools,
            description: "Dynamically discover and inspect specialized MCP database tools by keyword query (e.g., 'locks', 'bloat', 'index') or category ('query', 'dba', 'write'). Use this when you need specialized tools beyond the core surface.",
            input_schema: object_schema(&[
                (
                    "query",
                    "string",
                    false,
                    "Free-text keyword to search tool names/descriptions, e.g. \"locks\" or \"bloat\".",
                ),
                (
                    "category",
                    "string",
                    false,
                    "Restrict to a tool category: \"query\", \"dba\", \"meta\", or \"write\".",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::RunDoctor,
            description: "Run diagnostic health checks on active database connection, permissions, session guards, and index status.",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::SetupConnection,
            description: "Automatically detect or configure a database connection. Scans environment variables, workspace files, and local settings, eliciting missing credentials when supported.",
            input_schema: object_schema(&[
                ("name", "string", false, "Profile name to assign/detect."),
                (
                    "url",
                    "string",
                    false,
                    "Full postgres:// connection URL; if given, host/port/dbname/user/password are ignored.",
                ),
                ("host", "string", false, "Database server hostname."),
                (
                    "port",
                    "number",
                    false,
                    "Database server port (default 5432).",
                ),
                ("dbname", "string", false, "Database name to connect to."),
                ("user", "string", false, "Database role/username."),
                ("password", "string", false, "Database role password."),
                (
                    "sslmode",
                    "string",
                    false,
                    "libpq sslmode value, e.g. \"disable\", \"require\", \"verify-full\".",
                ),
                (
                    "interactive",
                    "boolean",
                    false,
                    "Prompt (elicit) for missing credentials instead of failing. Default false.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::SaveProfile,
            description: "Save or update a database connection profile in user configuration with atomic backup and dynamic session reload.",
            input_schema: object_schema(&[
                ("name", "string", true, "Profile name to save under."),
                (
                    "url",
                    "string",
                    false,
                    "Full postgres:// connection URL; if given, host/port/dbname/user/password are ignored.",
                ),
                ("host", "string", false, "Database server hostname."),
                (
                    "port",
                    "number",
                    false,
                    "Database server port (default 5432).",
                ),
                ("dbname", "string", false, "Database name to connect to."),
                ("user", "string", false, "Database role/username."),
                ("password", "string", false, "Database role password."),
                (
                    "sslmode",
                    "string",
                    false,
                    "libpq sslmode value, e.g. \"disable\", \"require\", \"verify-full\".",
                ),
                (
                    "access_mode",
                    "string",
                    false,
                    "Session access mode: \"read\", \"write\", or \"admin\". Setting \"write\" or \"admin\" requires confirm_elevated_access: true, or the call is rejected.",
                ),
                (
                    "confirm_elevated_access",
                    "boolean",
                    false,
                    "Required (must be true) when access_mode is \"write\" or \"admin\" — explicit opt-in for a privilege escalation. No effect when access_mode is omitted or \"read\".",
                ),
                (
                    "max_rows",
                    "number",
                    false,
                    "Row cap applied to run_select/export_query for this profile.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::TestProfile,
            description: "Test a database connection profile or inline parameters and return server version, superuser status, and round-trip latency.",
            input_schema: object_schema(&[
                (
                    "name",
                    "string",
                    false,
                    "Existing profile name to test. Omit to test inline parameters instead.",
                ),
                (
                    "url",
                    "string",
                    false,
                    "Full postgres:// connection URL to test inline (alternative to name).",
                ),
                (
                    "host",
                    "string",
                    false,
                    "Database server hostname (inline test).",
                ),
                (
                    "port",
                    "number",
                    false,
                    "Database server port (inline test).",
                ),
                ("dbname", "string", false, "Database name (inline test)."),
                (
                    "user",
                    "string",
                    false,
                    "Database role/username (inline test).",
                ),
                (
                    "password",
                    "string",
                    false,
                    "Database role password (inline test).",
                ),
                (
                    "sslmode",
                    "string",
                    false,
                    "libpq sslmode value (inline test), e.g. \"require\".",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::ExportProfile,
            description: "Export a secret-sanitized TOML configuration for team sharing (.nexql/config.toml) with all passwords and credentials stripped.",
            input_schema: object_schema(&[(
                "format",
                "string",
                false,
                "Output format, currently only \"toml\" is supported (default).",
            )]),
        },
        ToolSpec {
            name: ToolName::ImportProfile,
            description: "Import a team configuration file (.nexql/config.toml) or TOML content into local user configuration.",
            input_schema: object_schema(&[
                (
                    "content",
                    "string",
                    false,
                    "Raw TOML content to import. Provide this or `path`, not both.",
                ),
                (
                    "path",
                    "string",
                    false,
                    "Filesystem path to a .nexql/config.toml file to import.",
                ),
            ]),
        },
    ]
}

/// Phase 3 index tools (require `nexql-mcp index build`).
pub fn phase3_index_tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: ToolName::ResolveTarget,
            description: "Autonomously find which connection/database matches a user's hint (a database name, environment, host fragment) and/or an object hint (a table/view name), searching across ALL configured connections and their indexed schemas. Call this FIRST whenever the request references a database, environment, or object that is not the current session context — before search_schema, before list_connections. When the match is unambiguous it switches the session context automatically and returns the resolved connection/database; only returns `ambiguous: true` with a candidate list when multiple equally-plausible matches exist, in which case surface those candidates to the user rather than guessing.",
            input_schema: object_schema(&[
                (
                    "hint",
                    "string",
                    false,
                    "Free-text hint about the target connection: database name, environment, or host fragment.",
                ),
                (
                    "objectHint",
                    "string",
                    false,
                    "Free-text hint about a table/view name expected to live in the target database.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::Orient,
            description: "One-call schema bootstrap digest: tables (columns, PK, row estimate), FK join edges (declared vs inferred), enum-like low-cardinality text columns, and degradation notes. Call this FIRST on an unfamiliar database — before search_schema, describe_object, or get_join_path — to build context in a single low-token round trip instead of many.",
            input_schema: object_schema(&[(
                "focus",
                "string",
                false,
                "Substring to filter tables/joins by ref (e.g. \"orders\"). Omit to summarize the whole indexed schema.",
            )]),
        },
        ToolSpec {
            name: ToolName::SearchSchema,
            description: "Search the live, auto-indexed database schema using natural language or keywords to find tables, views, materialized views, and functions matching the query. Call this FIRST before writing any SQL — do not assume a table exists without finding it here.",
            input_schema: object_schema(&[(
                "query",
                "string",
                true,
                "Natural-language or keyword search, e.g. \"customer email\".",
            )]),
        },
        ToolSpec {
            name: ToolName::DescribeObject,
            description: "Get structural details of a specific database object (table, view, or materialized view) including columns, data types, constraints, and indexes.",
            input_schema: object_schema(&[(
                "ref",
                "string",
                true,
                "Object reference. Prefer schema-qualified form \"schema.name\" (e.g. \"public.customers\"); a bare name is resolved if unambiguous.",
            )]),
        },
        ToolSpec {
            name: ToolName::GetJoinPath,
            description: "Find the shortest path of join relationships and foreign keys between two database tables.",
            input_schema: object_schema(&[
                (
                    "a",
                    "string",
                    true,
                    "Source table reference. Prefer schema-qualified form \"schema.name\" (e.g. \"public.orders\"); a bare name is resolved if unambiguous.",
                ),
                (
                    "b",
                    "string",
                    true,
                    "Target table reference. Prefer schema-qualified form \"schema.name\" (e.g. \"public.customers\"); a bare name is resolved if unambiguous.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::SampleValues,
            description: "Retrieve a list of sample values from a specific table column to inspect its contents. Only works on read-only SELECT queries.",
            input_schema: object_schema(&[
                (
                    "ref",
                    "string",
                    true,
                    "Table/view reference. Prefer schema-qualified form \"schema.name\" (e.g. \"public.orders\"); a bare name is resolved if unambiguous.",
                ),
                (
                    "col",
                    "string",
                    true,
                    "Column name within `ref` to sample values from.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::RebuildIndex,
            description: "Rebuild the schema index for the active database connection.",
            input_schema: object_schema(&[(
                "depth",
                "string",
                false,
                "Index scope: \"shallow\" (structure only) or \"full\" (structure + sample values). Default \"full\".",
            )]),
        },
        ToolSpec {
            name: ToolName::RefreshIndex,
            description: "Refresh the schema index for the active database connection using previous build scope.",
            input_schema: object_schema(&[]),
        },
    ]
}

/// Phase 4 monitoring / DDL tools (descriptions from ToolSpec.ts where available).
pub fn phase4_tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: ToolName::GetDdl,
            description: "Get the DDL / definition of a database object. Views, materialized views, functions, and indexes return their CREATE statement; tables return structured DDL (columns, constraints, indexes).",
            input_schema: object_schema(&[
                (
                    "ref",
                    "string",
                    true,
                    "Object reference. Prefer schema-qualified form \"schema.name\" (e.g. \"public.orders\"); a bare name is resolved if unambiguous.",
                ),
                (
                    "kind",
                    "string",
                    false,
                    "Object kind hint: \"table\", \"view\", \"materialized_view\", \"function\", or \"index\". Auto-detected if omitted.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::TableStats,
            description: "Get size, row-count, activity (scans, inserts/updates/deletes, dead tuples, vacuum/analyze times) and per-column statistics for a specific table.",
            input_schema: object_schema(&[(
                "ref",
                "string",
                true,
                "Table reference. Prefer schema-qualified form \"schema.name\" (e.g. \"public.orders\"); a bare name is resolved if unambiguous.",
            )]),
        },
        ToolSpec {
            name: ToolName::IndexUsage,
            description: "Get index usage statistics (scan counts, size, definition, type) for a specific table's indexes. Useful for finding unused or missing indexes.",
            input_schema: object_schema(&[(
                "ref",
                "string",
                true,
                "Table reference. Prefer schema-qualified form \"schema.name\" (e.g. \"public.orders\"); a bare name is resolved if unambiguous.",
            )]),
        },
        ToolSpec {
            name: ToolName::ListRunningQueries,
            description: "List currently executing (non-idle) queries in the connected database with pid, user, state, wait events, and duration.",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::FindBlockingLocks,
            description: "Find lock contention: which queries are blocked waiting on locks and which pids/queries are blocking them.",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::SlowQueries,
            description: "List the slowest statements by mean execution time from pg_stat_statements (requires the extension; returns a hint if not installed).",
            input_schema: object_schema(&[(
                "limit",
                "number",
                false,
                "Maximum number of statements to return. Default 10.",
            )]),
        },
        ToolSpec {
            name: ToolName::DbHealthCheck,
            description: "Run a database health overview: size/connection stats, cache hit ratio, tables with dead tuples needing vacuum, active connections, and blocking-lock count. Sections that fail are reported individually; partial results are still returned.",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::GetIndexStatus,
            description: "Return schema-index status for the active connection/database: indexed_at, fingerprint, object counts, and optional live fingerprint drift. Returns status:\"missing\" (not an error) if no index has been built yet.",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::ListExtensions,
            description: "List installed PostgreSQL extensions (name, version, schema).",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::ServerSettings,
            description: "Return key PostgreSQL server settings from pg_settings (memory, connections, timeouts, autovacuum, version).",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::SuggestIndexes,
            description: "Suggest indexes from high sequential-scan tables, unindexed FK columns, and optional pg_stat_statements / EXPLAIN plan heuristics. Pass sql to analyze a specific query plan.",
            input_schema: object_schema(&[
                (
                    "limit",
                    "number",
                    false,
                    "Maximum number of suggestions to return. Default 10.",
                ),
                (
                    "sql",
                    "string",
                    false,
                    "Optional SELECT/WITH statement whose plan should inform the suggestions.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::FindUnusedIndexes,
            description: "List indexes with idx_scan = 0 (never used since stats reset), excluding primary keys, unique indexes, and constraint-backed indexes.",
            input_schema: object_schema(&[(
                "limit",
                "number",
                false,
                "Maximum number of indexes to return. Default 10.",
            )]),
        },
        ToolSpec {
            name: ToolName::BloatReport,
            description: "Approximate table bloat via dead-tuple ratio from pg_stat_user_tables (simplified estimate — not physical page bloat). Tables with >1000 dead tuples, ordered by bloat %.",
            input_schema: object_schema(&[(
                "limit",
                "number",
                false,
                "Maximum number of tables to return. Default 10.",
            )]),
        },
        ToolSpec {
            name: ToolName::FindMissingFks,
            description: "Find likely missing foreign keys: prefers schema-index join-graph inferred edges; falls back to catalog naming (*_id columns without an FK matching a PK).",
            input_schema: object_schema(&[(
                "limit",
                "number",
                false,
                "Maximum number of candidates to return. Default 20.",
            )]),
        },
    ]
}

/// Phase 4b read-only breadth (export / role introspection).
pub fn phase4b_tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: ToolName::ExportQuery,
            description: "Run a read-only SELECT/WITH and format results as CSV, JSON, or SQL INSERT statements. Honors max-row / max-char caps. For sqlinsert, pass table as schema.name.",
            input_schema: object_schema(&[
                (
                    "sql",
                    "string",
                    true,
                    "A single SELECT or WITH statement to run and export.",
                ),
                (
                    "format",
                    "string",
                    false,
                    "Output format: \"csv\", \"json\", or \"sqlinsert\". Default \"csv\".",
                ),
                (
                    "table",
                    "string",
                    false,
                    "Target table as \"schema.name\", required when format=\"sqlinsert\".",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::ListRoles,
            description: "List PostgreSQL roles (attributes). Pass role to get memberships and table privileges for one role.",
            input_schema: object_schema(&[(
                "role",
                "string",
                false,
                "Specific role name to inspect memberships/privileges for. Omit to list all roles.",
            )]),
        },
        ToolSpec {
            name: ToolName::DbDashboard,
            description: "One-shot live metrics bundle: DB size/owner, connection-state breakdown, top tables by size, object counts, active queries, and blocking locks. Soft-fails per section.",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::DeepPlanAnalysis,
            description: "Run EXPLAIN (ANALYZE by default) and return parsed plan metrics (scan counts, bottlenecks, buffer stats) plus severity-graded findings: estimate skew, expensive function/CTE/subquery nodes, and recommendations. Set analyze=false for plan-only (no execution). The single query-plan-analysis tool — covers what separate explain_analyze/analyze_query_plan tools used to.",
            input_schema: object_schema(&[
                (
                    "sql",
                    "string",
                    true,
                    "A single SELECT or WITH statement to analyze.",
                ),
                (
                    "analyze",
                    "boolean",
                    false,
                    "If false, use plan-only estimates without executing the query. Default true.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::SchemaDiff,
            description: "Compare two schemas in the current database (or sourceSchema vs targetSchema). Returns structured table/column/constraint/index diffs. Read-only — does not apply changes.",
            input_schema: object_schema(&[
                (
                    "sourceSchema",
                    "string",
                    true,
                    "Name of the schema to treat as the baseline, e.g. \"public\".",
                ),
                (
                    "targetSchema",
                    "string",
                    true,
                    "Name of the schema to diff against the baseline, e.g. \"staging\".",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::GenerateMigration,
            description: "Emit migration SQL to evolve sourceSchema toward targetSchema (from a live schema_diff). Read-only — returns SQL text, never executes it. Destructive drops are commented out.",
            input_schema: object_schema(&[
                (
                    "sourceSchema",
                    "string",
                    true,
                    "Name of the schema to migrate from, e.g. \"public\".",
                ),
                (
                    "targetSchema",
                    "string",
                    true,
                    "Name of the schema to migrate towards, e.g. \"staging\".",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::AutoTuneQuery,
            description: "Autonomous query tuner: executes EXPLAIN ANALYZE, checks table statistics, evaluates missing indexes, and outputs step-by-step performance tuning recommendations.",
            input_schema: object_schema(&[(
                "sql",
                "string",
                true,
                "A single SELECT or WITH statement to tune. It executes for real.",
            )]),
        },
        ToolSpec {
            name: ToolName::CheckDdlSafety,
            description: "Safety guard for migration DDL: inspects SQL for dangerous exclusive locks (e.g. non-concurrent index builds, column drops, table rewrites) and outputs risk scores and safe zero-downtime alternatives.",
            input_schema: object_schema(&[(
                "ddl",
                "string",
                true,
                "One or more DDL statements to inspect for locking risk. Not executed.",
            )]),
        },
    ]
}

/// Phase 9 write/admin tools (always listed; access-gated at dispatch).
pub fn phase9_write_tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: ToolName::ExecuteSql,
            description: "Execute DML (and DDL in admin mode) inside an explicit transaction. Set dry_run=true to roll back after execution. Errors always roll back.",
            input_schema: object_schema(&[
                (
                    "sql",
                    "string",
                    true,
                    "A single DML statement (INSERT/UPDATE/DELETE), or DDL if the session is in admin mode.",
                ),
                (
                    "dry_run",
                    "boolean",
                    false,
                    "If true, execute then roll back so no change persists. Default false.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::EditRow,
            description: "Structured insert, update, or delete by primary key. The server builds parameterized SQL — pass table (schema.name), action, values, and pk for update/delete.",
            input_schema: object_schema(&[
                ("table", "string", true, "Target table as \"schema.name\"."),
                (
                    "action",
                    "string",
                    true,
                    "Operation to perform: \"insert\", \"update\", or \"delete\".",
                ),
                (
                    "values",
                    "object",
                    false,
                    "Column name/value pairs to insert or update. Required for insert/update.",
                ),
                (
                    "pk",
                    "object",
                    false,
                    "Primary-key column name/value pairs identifying the row. Required for update/delete.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::ImportData,
            description: "Batch INSERT rows from a JSON array of objects into a table. Optional columns array fixes column order; otherwise keys from the first row are used.",
            input_schema: object_schema(&[
                ("table", "string", true, "Target table as \"schema.name\"."),
                (
                    "rows",
                    "array",
                    true,
                    "Array of row objects, each mapping column name to value.",
                ),
                (
                    "columns",
                    "array",
                    false,
                    "Explicit column order to insert with. Defaults to the keys of the first row.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::ApplyDdl,
            description: "Apply a DDL statement (CREATE, ALTER, DROP, TRUNCATE, …) in admin mode inside a transaction. Set dry_run=true to roll back.",
            input_schema: object_schema(&[
                ("sql", "string", true, "A single DDL statement to apply."),
                (
                    "dry_run",
                    "boolean",
                    false,
                    "If true, execute then roll back so no change persists. Default false.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::CreateIndexConcurrently,
            description: "Run CREATE INDEX CONCURRENTLY outside a transaction (non-blocking index build). Admin mode only.",
            input_schema: object_schema(&[(
                "sql",
                "string",
                true,
                "A single CREATE INDEX CONCURRENTLY statement.",
            )]),
        },
        ToolSpec {
            name: ToolName::RunMaintenance,
            description: "Run VACUUM, ANALYZE, or REINDEX outside a transaction. Admin mode only. Optional table (schema.name); vacuum supports full=true.",
            input_schema: object_schema(&[
                (
                    "action",
                    "string",
                    true,
                    "Maintenance action: \"vacuum\", \"analyze\", or \"reindex\".",
                ),
                (
                    "table",
                    "string",
                    false,
                    "Target table as \"schema.name\". Omit to run against the whole database where supported.",
                ),
                (
                    "full",
                    "boolean",
                    false,
                    "For action=\"vacuum\", run VACUUM FULL (rewrites the table, takes an exclusive lock). Default false.",
                ),
            ]),
        },
        ToolSpec {
            name: ToolName::TerminateQuery,
            description: "Cancel (pg_cancel_backend) or force-terminate (pg_terminate_backend) a backend by pid. Admin mode only. Refuses superuser targets and the current session.",
            input_schema: object_schema(&[
                (
                    "pid",
                    "number",
                    true,
                    "Backend process id to cancel/terminate.",
                ),
                (
                    "force",
                    "boolean",
                    false,
                    "If true, force-terminate the backend (pg_terminate_backend) instead of a soft cancel. Default false.",
                ),
            ]),
        },
    ]
}

/// Full tools/list surface for the current phase (catalog + index + Phase 4 + 4b + 9).
pub fn active_tools() -> Vec<ToolSpec> {
    let mut specs = phase2_catalog_tools();
    specs.extend(phase3_index_tools());
    specs.extend(phase4_tools());
    specs.extend(phase4b_tools());
    specs.extend(phase9_write_tools());
    specs
}

/// Build a JSON Schema object for a tool's input, from `(name, type, required, description)`
/// tuples. Every property carries a non-empty description (see `all_tools_have_descriptions`
/// test) and the object forbids unknown properties so a typo'd/hallucinated argument fails
/// loudly instead of silently vanishing.
fn object_schema(props: &[(&str, &str, bool, &str)]) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, ty, req, description) in props {
        let mut prop_val = match *ty {
            "array" => match *name {
                "columns" => json!({ "type": "array", "items": { "type": "string" } }),
                "rows" => json!({ "type": "array", "items": { "type": "object" } }),
                _ => json!({ "type": "array", "items": {} }),
            },
            _ => json!({ "type": *ty }),
        };
        prop_val["description"] = json!(*description);
        properties.insert((*name).into(), prop_val);
        if *req {
            required.push(json!(*name));
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolName;

    #[test]
    fn active_tools_lists_fifty_three() {
        let specs = active_tools();
        assert_eq!(specs.len(), 52);
        assert_eq!(specs.len(), ToolName::ACTIVE.len());
        for (spec, name) in specs.iter().zip(ToolName::ACTIVE.iter()) {
            assert_eq!(spec.name, *name);
        }
    }

    #[test]
    fn phase9_write_tools_count() {
        assert_eq!(phase9_write_tools().len(), ToolName::PHASE9.len());
    }

    #[test]
    fn array_properties_have_items() {
        for tool in active_tools() {
            if let Some(props) = tool
                .input_schema
                .get("properties")
                .and_then(|p| p.as_object())
            {
                for (prop_name, prop_val) in props {
                    if prop_val.get("type").and_then(|t| t.as_str()) == Some("array") {
                        assert!(
                            prop_val.get("items").is_some(),
                            "Tool '{}' parameter '{}' is array type but missing 'items'",
                            tool.name.as_str(),
                            prop_name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn profile_tools_filtering() {
        let query_specs = tools_for_profile(ToolProfile::Query);
        assert_eq!(query_specs.len(), 19);

        let dba_specs = tools_for_profile(ToolProfile::Dba);
        assert_eq!(dba_specs.len(), 26);

        let meta_specs = tools_for_profile(ToolProfile::Meta);
        assert_eq!(meta_specs.len(), 11);

        let full_specs = tools_for_profile(ToolProfile::Full);
        assert_eq!(full_specs.len(), 52);
    }

    /// Regression guard for Issue 1: every tool parameter must carry a non-empty
    /// `description`, and every input schema must forbid unknown properties.
    #[test]
    fn all_tools_have_descriptions_and_reject_unknown_properties() {
        for tool in active_tools() {
            assert_eq!(
                tool.input_schema.get("additionalProperties"),
                Some(&json!(false)),
                "Tool '{}' input_schema must set additionalProperties: false",
                tool.name.as_str()
            );
            if let Some(props) = tool
                .input_schema
                .get("properties")
                .and_then(|p| p.as_object())
            {
                for (prop_name, prop_val) in props {
                    let desc = prop_val.get("description").and_then(|d| d.as_str());
                    assert!(
                        desc.is_some_and(|d| !d.is_empty()),
                        "Tool '{}' parameter '{}' is missing a non-empty description",
                        tool.name.as_str(),
                        prop_name
                    );
                }
            }
        }
    }

    #[test]
    fn generate_mermaid_erd_test() {
        let obj = json!({
            "ref": "public.users",
            "columns": [
                { "name": "id", "type": "uuid", "is_pk": true, "is_fk": false },
                { "name": "email", "type": "varchar", "is_pk": false, "is_fk": false },
                { "name": "org_id", "type": "uuid", "is_pk": false, "is_fk": true }
            ]
        });
        let diagram = generate_mermaid_erd_for_object(obj.as_object().unwrap()).unwrap();
        assert!(diagram.contains("erDiagram"));
        assert!(diagram.contains("public_users"));
        assert!(diagram.contains("uuid id PK"));
        assert!(diagram.contains("uuid org_id FK"));
    }

    #[test]
    fn generate_mermaid_diagram_for_path_test() {
        let path = json!([
            { "from": "public.orders", "to": "public.users", "from_col": "user_id", "to_col": "id" }
        ]);
        let diagram = generate_mermaid_diagram_for_path(&path).unwrap();
        assert!(diagram.contains("erDiagram"));
        assert!(diagram.contains("public_orders }|--|| public_users : \"user_id -> id\""));
    }
}
