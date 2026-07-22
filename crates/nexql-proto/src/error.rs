use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtoError {
    #[error("unsupported MCP method: {0}")]
    UnknownMethod(String),

    #[error("invalid JSON-RPC request: {0}")]
    InvalidRequest(String),
}
