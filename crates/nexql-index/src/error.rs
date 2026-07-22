use thiserror::Error;

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("index not found at {0}")]
    NotFound(String),

    #[error("index format version mismatch: expected {expected}, got {actual}")]
    FormatVersion { expected: u32, actual: u32 },
}
