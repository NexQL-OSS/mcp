//! Tool catalog and executors.
//!
//! Tools return typed results; the protocol layer serializes them.
//! This crate must NOT depend on `nexql-proto`.

pub mod error;
pub mod exec;
pub mod registry;
pub mod schema;
pub mod session;

pub use error::ToolError;
pub use exec::{ToolOutcome, ToolRouter};
pub use registry::ToolName;
pub use schema::{ToolSpec, active_tools, phase2_catalog_tools, phase3_index_tools};
pub use session::{ConnectionInfo, ToolSession, default_index_root};
