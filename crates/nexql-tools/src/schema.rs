//! Phase 2 catalog tool descriptors (no index required).

use serde_json::{Value, json};

use crate::registry::ToolName;

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: ToolName,
    pub description: &'static str,
    pub input_schema: Value,
}

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
