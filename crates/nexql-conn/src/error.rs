// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

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

    #[error("postgres error: {}", format_pg_err(.0))]
    Postgres(#[from] tokio_postgres::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

fn format_pg_err(e: &tokio_postgres::Error) -> String {
    if let Some(db_err) = e.as_db_error() {
        let mut msg = db_err.message().to_string();
        if let Some(detail) = db_err.detail() {
            msg.push_str(&format!(" ({detail})"));
        }
        if let Some(hint) = db_err.hint() {
            msg.push_str(&format!(" [hint: {hint}]"));
        }
        msg
    } else {
        e.to_string()
    }
}
