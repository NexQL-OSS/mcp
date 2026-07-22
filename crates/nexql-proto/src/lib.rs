//! MCP protocol layer — JSON-RPC types and transports.
//!
//! Reference: `nexql-pro/pro/src/mcp/NexqlMcpServer.ts`

pub mod error;
pub mod types;

pub use error::ProtoError;

/// MCP protocol versions negotiated at `initialize` (newest first).
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Server instructions injected at `initialize` — keep verbatim from NexqlMcpServer.ts.
pub const MCP_SERVER_INSTRUCTIONS: &str = "\
NexQL exposes the REAL, live-indexed schema of the connected Postgres database.
Never invent or assume table, view, or column names from prior knowledge or naming conventions.
Before writing any SQL (run_select / explain_query), you MUST first ground it in the actual schema:
  1. list_schemas / list_objects or search_schema to find candidate objects.
  2. describe_object on each referenced table/view to confirm exact columns and types.
  3. get_join_path if the query spans multiple tables.
Only after confirming an object and its columns exist via these tools may you reference them in SQL.
If a table you expect is not returned by search_schema/list_objects, it does not exist — ask the user or pick from what was returned instead of guessing.";
