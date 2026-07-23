use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConnError {
    #[error("no connection source resolved")]
    NoSource,

    #[error("profile not found: {0}")]
    ProfileNotFound(String),

    #[error("invalid connection URL: {0}")]
    InvalidUrl(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("password_command failed: {0}")]
    PasswordCommand(String),

    #[error("password_command produced empty stdout")]
    EmptyPasswordCommand,

    #[error("env-file error: {0}")]
    EnvFile(String),

    #[error("pgpass error: {0}")]
    PgPass(String),

    #[error("pool error: {0}")]
    Pool(String),

    #[error("postgres error: {0}")]
    Postgres(#[from] tokio_postgres::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
