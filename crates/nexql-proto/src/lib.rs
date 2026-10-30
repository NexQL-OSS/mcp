// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! MCP protocol layer — JSON-RPC types and transports.
//!
//! Decision: hand-rolled for Phase 2 (see `docs/PROTO_DECISION.md`).
//! Reference: `nexql-pro/pro/src/mcp/NexqlMcpStdioHost.ts` and
//! `nexql-pro/pro/src/mcp/McpDefinitionProvider.ts`

pub mod backend;
pub mod error;
pub mod handler;
pub mod http;
pub mod stdio;
pub mod types;

pub use backend::{
    ClientCapabilities, ClientRequester, CompletionBackend, PromptBackend, ResourceBackend,
    RpcFailure, ToolAnnotations, ToolBackend, ToolCallResult, ToolDescriptor,
};
pub use error::ProtoError;
pub use handler::McpHandler;
pub use http::{HttpAuth, HttpServer};
pub use stdio::StdioServer;
pub use types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, negotiate_protocol_version};

/// MCP protocol versions negotiated at `initialize` (newest first).
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Server instructions injected at `initialize`. One discovery path, the one
/// models follow on their own: search, describe, then query.
pub const MCP_SERVER_INSTRUCTIONS: &str = "\
NexQL exposes the REAL, live-indexed schema of the connected Postgres database.
Never invent or assume table, view, or column names from prior knowledge or naming conventions.
Before writing any SQL (run_select / explain_query), ground it in the actual schema:
  1. search_schema with the user's words to find candidate tables and views.
  2. describe_object on each table/view you will use, to confirm exact columns and types.
  3. get_join_path if the query spans tables and the join is not obvious from foreign keys.
  4. run_select.
For a broad question about the whole database (\"what's in here?\"), call orient once instead of step 1.
If the data may live in another database, call search_all_databases, then pass its `database` to the other tools.
To connect another database, call add_database with its host, port, database and user: the user types the password on a page it opens, never in the chat. Never ask for a password, put one in a shell command, or write one to a file.
If a table you expect is not returned, it does not exist — ask the user or pick from what was returned instead of guessing.
Some columns (emails, phones, names, secrets) may come back masked as <redacted>; tell the user rather than trying to work around it.
save_profile merges into the existing named profile by default — omitted fields are preserved. Pass replace: true to wholesale-replace instead.
CREATE DATABASE / DROP DATABASE are always rejected in every access mode — this server manages objects within a connected database, not the database catalog. Do not retry with a higher access_mode.
For bulk INSERT/UPDATE/DDL that may exceed the default statement timeout, pass timeout_ms rather than retrying repeatedly.
apply_ddl asks the human directly (via MCP elicitation) before running a CRITICAL-risk statement when your client supports it — do not pre-emptively pass confirm_destructive: true to skip that prompt, and do not retry after a human declines.";
