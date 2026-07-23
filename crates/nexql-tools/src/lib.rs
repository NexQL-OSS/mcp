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
pub use schema::{ToolSpec, phase2_catalog_tools};
pub use session::{ConnectionInfo, ToolSession};
