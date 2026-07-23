//! Tool catalog and executors.
//!
//! Tools return typed results; the protocol layer serializes them.
//! This crate must NOT depend on `nexql-proto`.

pub mod completions;
pub mod error;
pub mod exec;
pub mod export;
pub mod plan;
pub mod prompts;
pub mod registry;
pub mod resources;
pub mod schema;
pub mod session;
pub mod sql;

pub use completions::{CompletionResult, CompletionsProvider};
pub use error::ToolError;
pub use exec::{ToolOutcome, ToolRouter};
pub use prompts::{PromptCatalog, PromptError, PromptGetResult, PromptInfo};
pub use registry::ToolName;
pub use resources::{
    McpResource, ResourceError, ResourceListResult, ResourceProvider, ResourceReadResult,
    decode_cursor, encode_cursor_state, parse_uri,
};
pub use schema::{
    ToolSpec, active_tools, phase2_catalog_tools, phase3_index_tools, phase4_tools, phase4b_tools,
};
pub use session::{ConnectionInfo, ToolSession, default_index_root};
