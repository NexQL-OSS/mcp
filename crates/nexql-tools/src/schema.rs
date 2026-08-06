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
            input_schema: object_schema(&[("connectionId", "string", true)]),
        },
        ToolSpec {
            name: ToolName::ListSchemas,
            description: "List non-system schemas in the currently selected database.",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::ListObjects,
            description: "List objects (tables, views, …) in a schema.",
            input_schema: object_schema(&[("schema", "string", false), ("kind", "string", false)]),
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
                ("connectionId", "string", true),
                ("database", "string", false),
            ]),
        },
        ToolSpec {
            name: ToolName::RunSelect,
            description: "Run a read-only SELECT or WITH query. DML/DDL are rejected. Only reference tables/columns confirmed via list_schemas / list_objects.",
            input_schema: object_schema(&[("sql", "string", true)]),
        },
        ToolSpec {
            name: ToolName::ExplainQuery,
            description: "Run EXPLAIN (no ANALYZE execute) for a SELECT/WITH query.",
            input_schema: object_schema(&[("sql", "string", true)]),
        },
        ToolSpec {
            name: ToolName::DiscoverTools,
            description: "Dynamically discover and inspect specialized MCP database tools by keyword query (e.g., 'locks', 'bloat', 'index') or category ('query', 'dba', 'write'). Use this when you need specialized tools beyond the core surface.",
            input_schema: object_schema(&[
                ("query", "string", false),
                ("category", "string", false),
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
            input_schema: object_schema(&[("hint", "string", false), ("objectHint", "string", false)]),
        },
        ToolSpec {
            name: ToolName::SearchSchema,
            description: "Search the live, auto-indexed database schema using natural language or keywords to find tables, views, materialized views, and functions matching the query. Call this FIRST before writing any SQL — do not assume a table exists without finding it here.",
            input_schema: object_schema(&[("query", "string", true)]),
        },
        ToolSpec {
            name: ToolName::DescribeObject,
            description: "Get structural details of a specific database object (table, view, or materialized view) including columns, data types, constraints, and indexes.",
            input_schema: object_schema(&[("ref", "string", true)]),
        },
        ToolSpec {
            name: ToolName::GetJoinPath,
            description: "Find the shortest path of join relationships and foreign keys between two database tables.",
            input_schema: object_schema(&[("a", "string", true), ("b", "string", true)]),
        },
        ToolSpec {
            name: ToolName::SampleValues,
            description: "Retrieve a list of sample values from a specific table column to inspect its contents. Only works on read-only SELECT queries.",
            input_schema: object_schema(&[("ref", "string", true), ("col", "string", true)]),
        },
    ]
}

/// Phase 4 monitoring / DDL tools (descriptions from ToolSpec.ts where available).
pub fn phase4_tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: ToolName::GetDdl,
            description: "Get the DDL / definition of a database object. Views, materialized views, functions, and indexes return their CREATE statement; tables return structured DDL (columns, constraints, indexes).",
            input_schema: object_schema(&[("ref", "string", true), ("kind", "string", false)]),
        },
        ToolSpec {
            name: ToolName::TableStats,
            description: "Get size, row-count, activity (scans, inserts/updates/deletes, dead tuples, vacuum/analyze times) and per-column statistics for a specific table.",
            input_schema: object_schema(&[("ref", "string", true)]),
        },
        ToolSpec {
            name: ToolName::IndexUsage,
            description: "Get index usage statistics (scan counts, size, definition, type) for a specific table's indexes. Useful for finding unused or missing indexes.",
            input_schema: object_schema(&[("ref", "string", true)]),
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
            input_schema: object_schema(&[("limit", "number", false)]),
        },
        ToolSpec {
            name: ToolName::DbHealthCheck,
            description: "Run a database health overview: size/connection stats, cache hit ratio, tables with dead tuples needing vacuum, active connections, and blocking-lock count. Sections that fail are reported individually; partial results are still returned.",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::ExplainAnalyze,
            description: "Run EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) on a SELECT/WITH query inside a read-only transaction that is always rolled back. WARNING: the query actually executes (volatile functions run), so expect real query runtime.",
            input_schema: object_schema(&[("sql", "string", true)]),
        },
        ToolSpec {
            name: ToolName::AnalyzeQueryPlan,
            description: "Run EXPLAIN (FORMAT JSON) on a SELECT/WITH query and return parsed plan metrics (scan counts, bottlenecks, buffer stats) plus performance recommendations. Set analyze=true to also execute the query for actual timings.",
            input_schema: object_schema(&[("sql", "string", true), ("analyze", "boolean", false)]),
        },
        ToolSpec {
            name: ToolName::GetIndexStatus,
            description: "Return schema-index status for the active connection/database: indexed_at, fingerprint, object counts, and optional live fingerprint drift.",
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
            input_schema: object_schema(&[("limit", "number", false), ("sql", "string", false)]),
        },
        ToolSpec {
            name: ToolName::FindUnusedIndexes,
            description: "List indexes with idx_scan = 0 (never used since stats reset), excluding primary keys, unique indexes, and constraint-backed indexes.",
            input_schema: object_schema(&[("limit", "number", false)]),
        },
        ToolSpec {
            name: ToolName::BloatReport,
            description: "Approximate table bloat via dead-tuple ratio from pg_stat_user_tables (simplified estimate — not physical page bloat). Tables with >1000 dead tuples, ordered by bloat %.",
            input_schema: object_schema(&[("limit", "number", false)]),
        },
        ToolSpec {
            name: ToolName::FindMissingFks,
            description: "Find likely missing foreign keys: prefers schema-index join-graph inferred edges; falls back to catalog naming (*_id columns without an FK matching a PK).",
            input_schema: object_schema(&[("limit", "number", false)]),
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
                ("sql", "string", true),
                ("format", "string", false),
                ("table", "string", false),
            ]),
        },
        ToolSpec {
            name: ToolName::ListRoles,
            description: "List PostgreSQL roles (attributes). Pass role to get memberships and table privileges for one role.",
            input_schema: object_schema(&[("role", "string", false)]),
        },
        ToolSpec {
            name: ToolName::DbDashboard,
            description: "One-shot live metrics bundle: DB size/owner, connection-state breakdown, top tables by size, object counts, active queries, and blocking locks. Soft-fails per section.",
            input_schema: object_schema(&[]),
        },
        ToolSpec {
            name: ToolName::DeepPlanAnalysis,
            description: "Run EXPLAIN (ANALYZE by default) and return severity-graded findings: estimate skew, expensive function/CTE/subquery nodes, and recommendations. Set analyze=false for plan-only (no execution).",
            input_schema: object_schema(&[("sql", "string", true), ("analyze", "boolean", false)]),
        },
        ToolSpec {
            name: ToolName::SchemaDiff,
            description: "Compare two schemas in the current database (or sourceSchema vs targetSchema). Returns structured table/column/constraint/index diffs. Read-only — does not apply changes.",
            input_schema: object_schema(&[
                ("sourceSchema", "string", true),
                ("targetSchema", "string", true),
            ]),
        },
        ToolSpec {
            name: ToolName::GenerateMigration,
            description: "Emit migration SQL to evolve sourceSchema toward targetSchema (from a live schema_diff). Read-only — returns SQL text, never executes it. Destructive drops are commented out.",
            input_schema: object_schema(&[
                ("sourceSchema", "string", true),
                ("targetSchema", "string", true),
            ]),
        },
        ToolSpec {
            name: ToolName::AutoTuneQuery,
            description: "Autonomous query tuner: executes EXPLAIN ANALYZE, checks table statistics, evaluates missing indexes, and outputs step-by-step performance tuning recommendations.",
            input_schema: object_schema(&[("sql", "string", true)]),
        },
        ToolSpec {
            name: ToolName::CheckDdlSafety,
            description: "Safety guard for migration DDL: inspects SQL for dangerous exclusive locks (e.g. non-concurrent index builds, column drops, table rewrites) and outputs risk scores and safe zero-downtime alternatives.",
            input_schema: object_schema(&[("ddl", "string", true)]),
        },
    ]
}

/// Phase 9 write/admin tools (always listed; access-gated at dispatch).
pub fn phase9_write_tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: ToolName::ExecuteSql,
            description: "Execute DML (and DDL in admin mode) inside an explicit transaction. Set dry_run=true to roll back after execution. Errors always roll back.",
            input_schema: object_schema(&[("sql", "string", true), ("dry_run", "boolean", false)]),
        },
        ToolSpec {
            name: ToolName::EditRow,
            description: "Structured insert, update, or delete by primary key. The server builds parameterized SQL — pass table (schema.name), action, values, and pk for update/delete.",
            input_schema: object_schema(&[
                ("table", "string", true),
                ("action", "string", true),
                ("values", "object", false),
                ("pk", "object", false),
            ]),
        },
        ToolSpec {
            name: ToolName::ImportData,
            description: "Batch INSERT rows from a JSON array of objects into a table. Optional columns array fixes column order; otherwise keys from the first row are used.",
            input_schema: object_schema(&[
                ("table", "string", true),
                ("rows", "array", true),
                ("columns", "array", false),
            ]),
        },
        ToolSpec {
            name: ToolName::ApplyDdl,
            description: "Apply a DDL statement (CREATE, ALTER, DROP, TRUNCATE, …) in admin mode inside a transaction. Set dry_run=true to roll back.",
            input_schema: object_schema(&[("sql", "string", true), ("dry_run", "boolean", false)]),
        },
        ToolSpec {
            name: ToolName::CreateIndexConcurrently,
            description: "Run CREATE INDEX CONCURRENTLY outside a transaction (non-blocking index build). Admin mode only.",
            input_schema: object_schema(&[("sql", "string", true)]),
        },
        ToolSpec {
            name: ToolName::RunMaintenance,
            description: "Run VACUUM, ANALYZE, or REINDEX outside a transaction. Admin mode only. Optional table (schema.name); vacuum supports full=true.",
            input_schema: object_schema(&[
                ("action", "string", true),
                ("table", "string", false),
                ("full", "boolean", false),
            ]),
        },
        ToolSpec {
            name: ToolName::TerminateQuery,
            description: "Cancel (pg_cancel_backend) or force-terminate (pg_terminate_backend) a backend by pid. Admin mode only. Refuses superuser targets and the current session.",
            input_schema: object_schema(&[("pid", "number", true), ("force", "boolean", false)]),
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

fn object_schema(props: &[(&str, &str, bool)]) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, ty, req) in props {
        let prop_val = match *ty {
            "array" => match *name {
                "columns" => json!({ "type": "array", "items": { "type": "string" } }),
                "rows" => json!({ "type": "array", "items": { "type": "object" } }),
                _ => json!({ "type": "array", "items": {} }),
            },
            _ => json!({ "type": *ty }),
        };
        properties.insert((*name).into(), prop_val);
        if *req {
            required.push(json!(*name));
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolName;

    #[test]
    fn active_tools_lists_forty_five() {
        let specs = active_tools();
        assert_eq!(specs.len(), 45);
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
        assert_eq!(query_specs.len(), 15);

        let dba_specs = tools_for_profile(ToolProfile::Dba);
        assert_eq!(dba_specs.len(), 25);

        let meta_specs = tools_for_profile(ToolProfile::Meta);
        assert_eq!(meta_specs.len(), 6);

        let full_specs = tools_for_profile(ToolProfile::Full);
        assert_eq!(full_specs.len(), 45);
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
