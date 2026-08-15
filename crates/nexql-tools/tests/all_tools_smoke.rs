// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Integration: call every active MCP tool with minimal valid args against temp PG.

mod common;

use common::smoke_env;
use nexql_tools::ToolName;
use serde_json::{Value, json};

/// Whether the tool call must succeed or only needs a structured response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmokeExpect {
    /// `!outcome.is_error`
    Success,
    /// Must not panic; may return a domain error (e.g. pid not found).
    Respond,
}

struct ToolSmokeCase {
    name: &'static str,
    args: Value,
    expect: SmokeExpect,
}

fn all_tool_cases(url: &str) -> Vec<ToolSmokeCase> {
    vec![
        ToolSmokeCase {
            name: "list_connections",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "list_databases",
            args: json!({ "connectionId": "default" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "list_schemas",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "list_objects",
            args: json!({ "schema": "public", "kind": "table" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "get_current_context",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "switch_connection",
            args: json!({ "connectionId": "default" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "run_select",
            args: json!({ "sql": "SELECT 1 AS n" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "explain_query",
            args: json!({ "sql": "SELECT 1" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "discover_tools",
            args: json!({ "query": "index" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "run_doctor",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "setup_connection",
            args: json!({ "name": "default", "url": url }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "save_profile",
            args: json!({
                "name": "smoke_alt",
                "host": "127.0.0.1",
                "dbname": "postgres",
                "user": "spike"
            }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "test_profile",
            args: json!({ "name": "default" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "export_profile",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "import_profile",
            args: json!({
                "content": "[profiles.imported]\nhost = \"127.0.0.1\"\ndbname = \"postgres\"\nuser = \"spike\"\n"
            }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "resolve_target",
            args: json!({ "objectHint": "users" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "orient",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "inspect_or_search",
            args: json!({ "query": "users" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "search_all_databases",
            args: json!({ "query": "users" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "search_schema",
            args: json!({ "query": "users" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "describe_object",
            args: json!({ "ref": "public.users" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "get_join_path",
            args: json!({ "a": "public.orders", "b": "public.users" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "sample_values",
            args: json!({ "ref": "public.users", "col": "email" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "rebuild_index",
            args: json!({ "depth": "shallow" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "refresh_index",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "get_ddl",
            args: json!({ "ref": "public.users" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "table_stats",
            args: json!({ "ref": "public.users" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "index_usage",
            args: json!({ "ref": "public.users" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "list_running_queries",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "find_blocking_locks",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "slow_queries",
            args: json!({ "limit": 5 }),
            // pg_stat_statements may be absent on throwaway PG.
            expect: SmokeExpect::Respond,
        },
        ToolSmokeCase {
            name: "db_health_check",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "get_index_status",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "list_extensions",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "server_settings",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "suggest_indexes",
            args: json!({ "limit": 5 }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "find_unused_indexes",
            args: json!({ "limit": 5 }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "bloat_report",
            args: json!({ "limit": 5 }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "find_missing_fks",
            args: json!({ "limit": 5 }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "export_query",
            args: json!({ "sql": "SELECT 1 AS n", "format": "json" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "list_roles",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "db_dashboard",
            args: json!({}),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "deep_plan_analysis",
            args: json!({ "sql": "SELECT 1", "analyze": false }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "schema_diff",
            args: json!({ "sourceSchema": "public", "targetSchema": "staging" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "generate_migration",
            args: json!({ "sourceSchema": "public", "targetSchema": "staging" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "auto_tune_query",
            args: json!({ "sql": "SELECT COUNT(*) FROM public.users" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "check_ddl_safety",
            args: json!({ "ddl": "CREATE INDEX ON public.users (email)" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "execute_sql",
            args: json!({
                "sql": "INSERT INTO public.users (email) VALUES ('smoke@test.example')",
                "dry_run": true
            }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "edit_row",
            args: json!({
                "table": "public.users",
                "action": "insert",
                "values": { "email": "editrow@test.example" },
                "dry_run": true
            }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "import_data",
            args: json!({ "table": "public.users", "rows": [] }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "apply_ddl",
            args: json!({
                "sql": "CREATE TABLE IF NOT EXISTS public._nexql_smoke_tmp (id int)",
                "dry_run": true
            }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "create_index_concurrently",
            args: json!({
                "sql": "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_nexql_smoke_users_email ON public.users (email)"
            }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "run_maintenance",
            args: json!({ "action": "analyze", "table": "public.users" }),
            expect: SmokeExpect::Success,
        },
        ToolSmokeCase {
            name: "terminate_query",
            args: json!({ "pid": 9_999_999 }),
            expect: SmokeExpect::Respond,
        },
    ]
}

#[tokio::test]
async fn all_active_tools_respond_to_minimal_calls() {
    let Some(env) = smoke_env().await else {
        eprintln!("skip: initdb/postgres unavailable or index build failed");
        return;
    };

    let cases = all_tool_cases(&env.url);
    assert_eq!(
        cases.len(),
        ToolName::ACTIVE.len(),
        "smoke table must cover every active tool"
    );

    let mut failures = Vec::new();

    for case in cases {
        let out = env.router.call(case.name, case.args.clone()).await;

        if out.text.is_empty() {
            failures.push(format!("{}: empty response text", case.name));
            continue;
        }

        if out.text.contains("unknown tool") || out.text.contains("Unknown tool") {
            failures.push(format!("{}: unknown tool: {}", case.name, out.text));
            continue;
        }

        match case.expect {
            SmokeExpect::Success if out.is_error => {
                failures.push(format!(
                    "{}: expected success, got error: {}",
                    case.name, out.text
                ));
            }
            SmokeExpect::Respond | SmokeExpect::Success => {}
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} of {} tool calls failed:\n{}",
            failures.len(),
            ToolName::ACTIVE.len(),
            failures.join("\n")
        );
    }
}

#[tokio::test]
async fn all_active_tool_schemas_are_mcp_valid() {
    use nexql_tools::schema::active_tools;

    fn items_schema_is_valid(items: &Value) -> bool {
        items.get("type").is_some()
            || items.get("oneOf").is_some()
            || items.get("anyOf").is_some()
            || items.get("allOf").is_some()
    }

    let mut failures = Vec::new();
    for tool in active_tools() {
        let Some(props) = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.as_object())
        else {
            continue;
        };
        for (prop_name, prop_val) in props {
            if prop_val.get("type").and_then(|t| t.as_str()) != Some("array") {
                continue;
            }
            let Some(items) = prop_val.get("items") else {
                failures.push(format!(
                    "{}: parameter '{}' is array without items",
                    tool.name.as_str(),
                    prop_name
                ));
                continue;
            };
            if !items_schema_is_valid(items) {
                failures.push(format!(
                    "{}: parameter '{}' has array items without a concrete schema",
                    tool.name.as_str(),
                    prop_name
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "MCP schema validation failures:\n{}",
        failures.join("\n")
    );
}
