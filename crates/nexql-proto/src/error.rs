// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtoError {
    #[error("invalid JSON-RPC: {0}")]
    InvalidRequest(String),

    #[error("method not found: {0}")]
    MethodNotFound(String),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl ProtoError {
    pub const PARSE: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL: i32 = -32603;
    /// MCP resource not found (application error).
    pub const RESOURCE_NOT_FOUND: i32 = -32002;

    pub fn code(&self) -> i32 {
        match self {
            Self::InvalidRequest(_) => Self::INVALID_REQUEST,
            Self::MethodNotFound(_) => Self::METHOD_NOT_FOUND,
            Self::InvalidParams(_) => Self::INVALID_PARAMS,
            Self::Io(_) | Self::Other(_) => Self::INTERNAL,
        }
    }
}
