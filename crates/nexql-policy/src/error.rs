use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("access denied: {0}")]
    Denied(String),

    #[error("SQL rejected: {0}")]
    SqlRejected(String),

    #[error("SQL parse error: {0}")]
    SqlParse(String),
}
