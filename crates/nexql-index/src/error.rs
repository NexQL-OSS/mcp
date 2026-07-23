use thiserror::Error;

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("index not found at {0}")]
    NotFound(String),

    #[error("index format version mismatch: expected {expected}, got {actual}")]
    FormatVersion { expected: u32, actual: u32 },

    #[error("manifest format version {actual} is newer than current {expected}")]
    FormatTooNew { expected: u32, actual: u32 },

    #[error("no migration path found from format version {0}")]
    NoMigrationPath(u32),

    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("index build already in progress (lock held) at {0}")]
    Locked(String),

    #[error("index build cancelled")]
    Cancelled,

    #[error("index build error: {0}")]
    Build(String),

    #[error("database error: {0}")]
    Db(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<tokio_postgres::Error> for IndexError {
    fn from(err: tokio_postgres::Error) -> Self {
        IndexError::Db(err.to_string())
    }
}
