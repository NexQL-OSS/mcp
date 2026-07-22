use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConnError {
    #[error("no connection source resolved")]
    NoSource,

    #[error("profile not found: {0}")]
    ProfileNotFound(String),
}
