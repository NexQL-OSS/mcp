use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("access denied: {0}")]
    Denied(String),
}
