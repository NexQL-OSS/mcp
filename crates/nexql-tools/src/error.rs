use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("unknown tool: {0}")]
    Unknown(String),

    #[error("invalid arguments: {0}")]
    InvalidArgs(String),

    #[error("tool execution failed: {0}")]
    Execution(String),

    #[error(transparent)]
    Conn(#[from] nexql_conn::ConnError),

    #[error(transparent)]
    Policy(#[from] nexql_policy::PolicyError),

    #[error(transparent)]
    Index(#[from] nexql_index::IndexError),

    #[error(transparent)]
    Postgres(#[from] tokio_postgres::Error),
}
