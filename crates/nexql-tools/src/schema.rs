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
    ]
}

/// Full tools/list surface for the current phase (catalog + index + Phase 4).
pub fn active_tools() -> Vec<ToolSpec> {
    let mut specs = phase2_catalog_tools();
    specs.extend(phase3_index_tools());
    specs.extend(phase4_tools());
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
    fn active_tools_lists_twenty_four() {
        let specs = active_tools();
        assert_eq!(specs.len(), 24);
        assert_eq!(specs.len(), ToolName::ACTIVE.len());
        for (spec, name) in specs.iter().zip(ToolName::ACTIVE.iter()) {
            assert_eq!(spec.name, *name);
        }
    }
}
