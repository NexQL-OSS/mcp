//! Tool descriptors for the active MCP surface (Phase 2–4).

use serde_json::{Value, json};

use crate::registry::ToolName;

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: ToolName,
    pub description: &'static str,
    pub input_schema: Value,
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
    ]
}

/// Phase 3 index tools (require `nexql-mcp index build`).
pub fn phase3_index_tools() -> Vec<ToolSpec> {
    vec![
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
    ]
}

/// Full tools/list surface for the current phase (catalog + index + Phase 4 + 4b).
pub fn active_tools() -> Vec<ToolSpec> {
    let mut specs = phase2_catalog_tools();
    specs.extend(phase3_index_tools());
    specs.extend(phase4_tools());
    specs.extend(phase4b_tools());
    specs
}

fn object_schema(props: &[(&str, &str, bool)]) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, ty, req) in props {
        properties.insert((*name).into(), json!({ "type": *ty }));
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
    fn active_tools_lists_thirty_two() {
        let specs = active_tools();
        assert_eq!(specs.len(), 32);
        assert_eq!(specs.len(), ToolName::ACTIVE.len());
        for (spec, name) in specs.iter().zip(ToolName::ACTIVE.iter()) {
            assert_eq!(spec.name, *name);
        }
    }
}
