// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Tool catalog and executors.
//!
//! Tools return typed results; the protocol layer serializes them.
//! This crate must NOT depend on `nexql-proto`.

pub mod cell_json;
pub mod completions;
pub mod dba_guard;
pub mod detect;
pub mod error;
pub mod exec;
pub mod export;
pub mod plan;
pub mod prompts;
pub mod registry;
pub mod resources;
pub mod schema;
pub mod schema_diff;
pub mod session;
pub mod sql;
pub mod write;

pub use completions::{CompletionResult, CompletionsProvider};
pub use detect::{ConnectionDetector, DetectedCandidate};
pub use error::ToolError;
pub use exec::{ToolOutcome, ToolRouter};
pub use prompts::{PromptCatalog, PromptError, PromptGetResult, PromptInfo};
pub use registry::{ToolName, ToolProfile};
pub use resources::{
    McpResource, ResourceError, ResourceListResult, ResourceProvider, ResourceReadResult,
    decode_cursor, encode_cursor_state, parse_uri,
};
pub use schema::{
    ToolSpec, active_tools, generate_mermaid_diagram_for_path, generate_mermaid_erd_for_object,
    phase2_catalog_tools, phase3_index_tools, phase4_tools, phase4b_tools, phase9_write_tools,
    tools_for_profile,
};
pub use session::{
    ConnectionInfo, ConnectionPolicy, ToolSession, default_index_root, policy_from_profile,
};
