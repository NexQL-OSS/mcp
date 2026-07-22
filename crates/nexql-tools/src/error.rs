use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("unknown tool: {0}")]
    Unknown(String),

    #[error("tool execution failed: {0}")]
    Execution(String),
}
