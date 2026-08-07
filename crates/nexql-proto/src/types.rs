// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ProtoError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: Option<String>,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcResponse {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    pub fn from_proto_err(id: Value, err: &ProtoError) -> Self {
        Self::err(id, err.code(), err.to_string())
    }
}

/// Negotiate client-requested protocol version against supported list.
pub fn negotiate_protocol_version(requested: Option<&str>, supported: &[&str]) -> &'static str {
    // supported is newest-first; return as &'static by matching constants.
    if let Some(req) = requested {
        for &v in supported {
            if v == req {
                return match v {
                    "2025-06-18" => "2025-06-18",
                    "2025-03-26" => "2025-03-26",
                    "2024-11-05" => "2024-11-05",
                    _ => "2025-06-18",
                };
            }
        }
    }
    "2025-06-18"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SUPPORTED_PROTOCOL_VERSIONS;

    #[test]
    fn negotiates_known_version() {
        assert_eq!(
            negotiate_protocol_version(Some("2024-11-05"), SUPPORTED_PROTOCOL_VERSIONS),
            "2024-11-05"
        );
    }

    #[test]
    fn unknown_version_picks_newest() {
        assert_eq!(
            negotiate_protocol_version(Some("1999-01-01"), SUPPORTED_PROTOCOL_VERSIONS),
            "2025-06-18"
        );
    }

    #[test]
    fn missing_version_picks_newest() {
        assert_eq!(
            negotiate_protocol_version(None, SUPPORTED_PROTOCOL_VERSIONS),
            "2025-06-18"
        );
    }
}
