//! Tool catalog and executors.
//!
//! Tools return typed results; the protocol layer serializes them.
//! This crate must NOT depend on `nexql-proto`.
//!
//! Reference:
//! - `nexql-pro/pro/src/providers/chat/tools/ToolSpec.ts`
//! - `nexql-pro/pro/src/providers/chat/tools/ToolExecutor.ts`
//! - `nexql-pro/pro/src/mcp/McpPrompts.ts`
//! - `nexql-pro/pro/src/mcp/McpResourceProvider.ts`

pub mod error;
pub mod registry;

pub use error::ToolError;
pub use registry::ToolName;
