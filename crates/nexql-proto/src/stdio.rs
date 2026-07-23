//! Stdio MCP server — newline-delimited JSON-RPC.

use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::MCP_SERVER_INSTRUCTIONS;
use crate::SUPPORTED_PROTOCOL_VERSIONS;
use crate::backend::ToolBackend;
use crate::error::ProtoError;
use crate::types::{JsonRpcRequest, JsonRpcResponse, negotiate_protocol_version};

pub struct StdioServer {
    backend: Arc<dyn ToolBackend>,
    server_name: String,
    server_version: String,
}

impl StdioServer {
    pub fn new(backend: Arc<dyn ToolBackend>) -> Self {
        Self {
            backend,
            server_name: "nexql-mcp".into(),
            server_version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    pub async fn serve<R, W>(self, reader: R, mut writer: W) -> Result<(), ProtoError>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut lines = BufReader::new(reader).lines();
        while let Some(line) = lines.next_line().await? {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let response = self.handle_line(line).await;
            if let Some(resp) = response {
                let mut out =
                    serde_json::to_string(&resp).map_err(|e| ProtoError::Other(e.to_string()))?;
                out.push('\n');
                writer.write_all(out.as_bytes()).await?;
                writer.flush().await?;
            }
        }
        Ok(())
    }

    async fn handle_line(&self, line: &str) -> Option<JsonRpcResponse> {
        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                return Some(JsonRpcResponse::err(
                    Value::Null,
                    ProtoError::PARSE,
                    format!("parse error: {e}"),
                ));
            }
        };

        // Notifications have no id — no response.
        let id = match req.id.clone() {
            Some(id) => id,
            None => {
                // Still process initialized notification etc.
                if req.method.as_deref() == Some("notifications/initialized") {
                    return None;
                }
                return None;
            }
        };

        let method = match req.method.as_deref() {
            Some(m) => m,
            None => {
                return Some(JsonRpcResponse::err(
                    id,
                    ProtoError::INVALID_REQUEST,
                    "missing method",
                ));
            }
        };

        match method {
            "initialize" => Some(self.handle_initialize(id, req.params)),
            "ping" => Some(JsonRpcResponse::ok(id, json!({}))),
            "tools/list" => Some(self.handle_tools_list(id).await),
            "tools/call" => Some(self.handle_tools_call(id, req.params).await),
            other => Some(JsonRpcResponse::err(
                id,
                ProtoError::METHOD_NOT_FOUND,
                format!("Method not found: {other}"),
            )),
        }
    }

    fn handle_initialize(&self, id: Value, params: Option<Value>) -> JsonRpcResponse {
        let requested = params
            .as_ref()
            .and_then(|p| p.get("protocolVersion"))
            .and_then(|v| v.as_str());
        let version = negotiate_protocol_version(requested, SUPPORTED_PROTOCOL_VERSIONS);
        JsonRpcResponse::ok(
            id,
            json!({
                "protocolVersion": version,
                "capabilities": {
                    "tools": { "listChanged": false }
                },
                "serverInfo": {
                    "name": self.server_name,
                    "version": self.server_version
                },
                "instructions": MCP_SERVER_INSTRUCTIONS
            }),
        )
    }

    async fn handle_tools_list(&self, id: Value) -> JsonRpcResponse {
        let tools = self.backend.list_tools().await;
        let tools_json: Vec<Value> = tools
            .into_iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema
                })
            })
            .collect();
        JsonRpcResponse::ok(id, json!({ "tools": tools_json }))
    }

    async fn handle_tools_call(&self, id: Value, params: Option<Value>) -> JsonRpcResponse {
        let Some(params) = params else {
            return JsonRpcResponse::err(id, ProtoError::INVALID_PARAMS, "missing params");
        };
        let name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => {
                return JsonRpcResponse::err(id, ProtoError::INVALID_PARAMS, "missing tool name");
            }
        };
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let result = self.backend.call_tool(name, args).await;
        let content = vec![json!({
            "type": "text",
            "text": result.text
        })];
        let mut body = json!({
            "content": content,
            "isError": result.is_error
        });
        if let Some(structured) = result.structured {
            body["structuredContent"] = structured;
        }
        JsonRpcResponse::ok(id, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{ToolCallResult, ToolDescriptor};
    use async_trait::async_trait;
    use std::io::Cursor;

    struct FakeBackend;

    #[async_trait]
    impl ToolBackend for FakeBackend {
        async fn list_tools(&self) -> Vec<ToolDescriptor> {
            vec![ToolDescriptor {
                name: "ping_tool".into(),
                description: "test".into(),
                input_schema: json!({"type":"object","properties":{}}),
            }]
        }

        async fn call_tool(&self, name: &str, _arguments: Value) -> ToolCallResult {
            ToolCallResult {
                text: format!("called {name}"),
                structured: Some(json!({"ok": true})),
                is_error: false,
            }
        }
    }

    #[tokio::test]
    async fn initialize_returns_verbatim_instructions() {
        let server = StdioServer::new(Arc::new(FakeBackend));
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name":"t","version":"0"} }
        });
        let resp = server.handle_line(&req.to_string()).await.unwrap();
        let result = resp.result.unwrap();
        assert_eq!(
            result.get("instructions").and_then(|v| v.as_str()).unwrap(),
            MCP_SERVER_INSTRUCTIONS
        );
        assert_eq!(
            result.get("protocolVersion").and_then(|v| v.as_str()),
            Some("2025-06-18")
        );
    }

    #[tokio::test]
    async fn unknown_method_is_32601() {
        let server = StdioServer::new(Arc::new(FakeBackend));
        let req = r#"{"jsonrpc":"2.0","id":2,"method":"nope"}"#;
        let resp = server.handle_line(req).await.unwrap();
        assert_eq!(resp.error.as_ref().unwrap().code, -32601);
    }

    #[tokio::test]
    async fn ping_and_tools_roundtrip_on_stdio() {
        let server = StdioServer::new(Arc::new(FakeBackend));
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
        );
        let reader = Cursor::new(input.as_bytes().to_vec());
        let mut writer = Vec::new();
        server.serve(reader, &mut writer).await.unwrap();
        let out = String::from_utf8(writer).unwrap();
        assert!(out.contains("\"id\":1"));
        assert!(out.contains("ping_tool"));
    }
}
