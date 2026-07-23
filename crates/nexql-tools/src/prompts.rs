//! Static MCP prompts — canned diagnostic workflows.
//!
//! Port of `pro/src/mcp/McpPrompts.ts`, plus Phase 4 additions:
//! `write-migration`, `optimize-table`, `explain-this-query`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const ERR_INVALID_PARAMS: i32 = -32602;

#[derive(Debug, Error)]
pub enum PromptError {
    #[error("{0}")]
    InvalidParams(String),
}

impl PromptError {
    pub fn code(&self) -> i32 {
        ERR_INVALID_PARAMS
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptInfo {
    pub name: String,
    pub description: String,
    pub arguments: Vec<PromptArgument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessageContent {
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    pub role: String,
    pub content: PromptMessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptGetResult {
    pub description: String,
    pub messages: Vec<PromptMessage>,
}

struct PromptDef {
    name: &'static str,
    description: &'static str,
    arguments: &'static [ArgDef],
    build: fn(&HashMap<String, String>) -> String,
}

struct ArgDef {
    name: &'static str,
    description: &'static str,
    required: bool,
}

const PROMPTS: &[PromptDef] = &[
    PromptDef {
        name: "health-check",
        description: "Run a full database health assessment and summarize issues by severity.",
        arguments: &[],
        build: |_| {
            [
                "Assess the health of the connected PostgreSQL database:",
                "1. Run the db_health_check tool for the overview (size, connections, cache hit ratio, dead tuples).",
                "2. Run find_blocking_locks to check for lock contention.",
                "3. Run list_running_queries to spot long-running or stuck queries.",
                "Then produce a summary grouped by severity (critical / warning / ok):",
                "- Flag cache hit ratio below 0.95, any blocking locks, queries running longer than 5 minutes, and tables with high dead-tuple counts.",
                "- For each issue, state the evidence and a concrete remediation (e.g. VACUUM, index, terminate pid).",
            ]
            .join("\n")
        },
    },
    PromptDef {
        name: "analyze-slow-queries",
        description: "Find the slowest queries and propose index or rewrite improvements.",
        arguments: &[],
        build: |_| {
            [
                "Identify and improve the slowest queries in the connected database:",
                "1. Run the slow_queries tool to get the top statements by mean execution time.",
                "2. For each of the top 3 offenders, run analyze_query_plan on the query text to get plan metrics and bottlenecks.",
                "3. Before proposing any index, verify the referenced tables and columns exist using describe_object.",
                "Deliver: for each slow query — the bottleneck (seq scan, spill, misestimate), a proposed fix (CREATE INDEX CONCURRENTLY statement or query rewrite), and the expected impact.",
            ]
            .join("\n")
        },
    },
    PromptDef {
        name: "explore-schema",
        description: "Explore and summarize the database schema around a topic.",
        arguments: &[ArgDef {
            name: "topic",
            description: "What to explore, e.g. \"orders\", \"user accounts\", \"billing\".",
            required: true,
        }],
        build: |args| {
            let topic = args.get("topic").map(String::as_str).unwrap_or("");
            [
                format!("Explore the database schema related to: {topic}"),
                "1. Run search_schema with the topic to find relevant tables, views, and functions.".into(),
                "2. Run describe_object on each of the top hits to get columns, keys, and indexes.".into(),
                "3. Run get_join_path between related tables to understand how they connect.".into(),
                "Deliver a schema summary: the core tables with their purpose, key columns, relationships (as a join diagram in text), and any views or functions that operate on them.".into(),
            ]
            .join("\n")
        },
    },
    PromptDef {
        name: "debug-blocking",
        description: "Diagnose lock contention and identify the root blocking session.",
        arguments: &[],
        build: |_| {
            [
                "Diagnose lock contention in the connected database:",
                "1. Run find_blocking_locks to get blocked/blocking pid pairs with their queries.",
                "2. Run list_running_queries to see the full activity picture (states, wait events, durations).",
                "Then explain the lock chain: which pid is the root blocker, what query it is running, how long it has been running, and which sessions are waiting on it (directly or transitively).",
                "Recommend an action: wait, or terminate the root blocker (give the exact pg_terminate_backend(pid) statement, but do NOT execute it — all tools are read-only).",
            ]
            .join("\n")
        },
    },
    PromptDef {
        name: "write-migration",
        description: "Draft a safe PostgreSQL migration for a described schema change.",
        arguments: &[ArgDef {
            name: "change",
            description: "The schema change to implement, e.g. \"add soft-delete to orders\".",
            required: true,
        }],
        build: |args| {
            let change = args.get("change").map(String::as_str).unwrap_or("");
            [
                format!("Draft a PostgreSQL migration for: {change}"),
                "1. Use search_schema and describe_object to ground every table/column you will touch in the live index.".into(),
                "2. Prefer non-blocking patterns (CREATE INDEX CONCURRENTLY, ADD COLUMN nullable first, backfill, then constrain).".into(),
                "3. Produce up and down SQL as separate scripts, with a short risk note (locks, rewrite, invalid indexes).".into(),
                "Do not run write SQL — tools are read-only; only propose statements.".into(),
            ]
            .join("\n")
        },
    },
    PromptDef {
        name: "optimize-table",
        description: "Analyze a table's indexes, bloat signals, and access patterns; propose improvements.",
        arguments: &[ArgDef {
            name: "ref",
            description: "Table ref as schema.name, e.g. \"public.orders\".",
            required: true,
        }],
        build: |args| {
            let ref_ = args.get("ref").map(String::as_str).unwrap_or("");
            [
                format!("Optimize table {ref_}:"),
                "1. Run describe_object on the ref to get columns, keys, and indexes.".into(),
                "2. Run table_stats and index_usage for the same ref.".into(),
                "3. Cross-check with slow_queries / analyze_query_plan for statements that hit this table.".into(),
                "Deliver: unused or redundant indexes, missing indexes (with CREATE INDEX CONCURRENTLY), and VACUUM/ANALYZE advice with evidence.".into(),
            ]
            .join("\n")
        },
    },
    PromptDef {
        name: "explain-this-query",
        description: "Explain a SQL query against the live schema and propose plan improvements.",
        arguments: &[ArgDef {
            name: "sql",
            description: "The SQL SELECT (or other read query) to explain.",
            required: true,
        }],
        build: |args| {
            let sql = args.get("sql").map(String::as_str).unwrap_or("");
            [
                "Explain and improve this query:".to_owned(),
                format!("```sql\n{sql}\n```"),
                "1. Ground every referenced object with describe_object / search_schema before commenting on columns.".into(),
                "2. Run explain_query (and analyze_query_plan if available) on the SQL.".into(),
                "3. Call out seq scans, misestimates, spills, and missing indexes; propose a rewritten query or index when justified.".into(),
            ]
            .join("\n")
        },
    },
];

/// Catalog of static MCP prompts.
pub struct PromptCatalog;

impl PromptCatalog {
    pub fn list() -> Vec<PromptInfo> {
        PROMPTS
            .iter()
            .map(|p| PromptInfo {
                name: p.name.into(),
                description: p.description.into(),
                arguments: p
                    .arguments
                    .iter()
                    .map(|a| PromptArgument {
                        name: a.name.into(),
                        description: a.description.into(),
                        required: a.required,
                    })
                    .collect(),
            })
            .collect()
    }

    pub fn get(
        name: &str,
        args: &HashMap<String, String>,
    ) -> Result<PromptGetResult, PromptError> {
        let prompt = PROMPTS
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| PromptError::InvalidParams(format!("Unknown prompt: {name}")))?;

        for arg in prompt.arguments {
            if arg.required {
                let missing = match args.get(arg.name) {
                    None => true,
                    Some(v) if v.is_empty() => true,
                    Some(_) => false,
                };
                if missing {
                    return Err(PromptError::InvalidParams(format!(
                        "Missing required argument \"{}\" for prompt \"{name}\"",
                        arg.name
                    )));
                }
            }
        }

        let text = (prompt.build)(args);
        Ok(PromptGetResult {
            description: prompt.description.into(),
            messages: vec![PromptMessage {
                role: "user".into(),
                content: PromptMessageContent {
                    type_: "text".into(),
                    text,
                },
            }],
        })
    }

    pub fn names() -> Vec<&'static str> {
        PROMPTS.iter().map(|p| p.name).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_seven_prompts_including_phase4() {
        let names = PromptCatalog::names();
        assert_eq!(names.len(), 7);
        for expected in [
            "health-check",
            "analyze-slow-queries",
            "explore-schema",
            "debug-blocking",
            "write-migration",
            "optimize-table",
            "explain-this-query",
        ] {
            assert!(names.contains(&expected), "missing {expected}");
        }
        let explore = PromptCatalog::list()
            .into_iter()
            .find(|p| p.name == "explore-schema")
            .unwrap();
        assert_eq!(explore.arguments[0].name, "topic");
        assert!(explore.arguments[0].required);
    }

    #[test]
    fn get_rejects_missing_required_arg() {
        let err = PromptCatalog::get("explore-schema", &HashMap::new()).unwrap_err();
        assert_eq!(err.code(), ERR_INVALID_PARAMS);
        assert!(err.to_string().contains("topic"));
    }

    #[test]
    fn get_unknown_prompt() {
        let err = PromptCatalog::get("nope", &HashMap::new()).unwrap_err();
        assert!(err.to_string().contains("Unknown prompt"));
    }

    #[test]
    fn get_debug_blocking_mentions_tool() {
        let result = PromptCatalog::get("debug-blocking", &HashMap::new()).unwrap();
        assert!(result.messages[0].content.text.contains("find_blocking_locks"));
    }
}
