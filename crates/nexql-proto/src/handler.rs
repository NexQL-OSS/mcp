//! Shared MCP JSON-RPC dispatch for stdio and HTTP transports.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::MCP_SERVER_INSTRUCTIONS;
use crate::SUPPORTED_PROTOCOL_VERSIONS;
use crate::backend::{CompletionBackend, PromptBackend, ResourceBackend, ToolBackend};
use crate::error::ProtoError;
use crate::types::{JsonRpcRequest, JsonRpcResponse, negotiate_protocol_version};

pub struct McpHandler {
    tools: Arc<dyn ToolBackend>,
    resources: Option<Arc<dyn ResourceBackend>>,
    prompts: Option<Arc<dyn PromptBackend>>,
    completions: Option<Arc<dyn CompletionBackend>>,
    server_name: String,
    server_version: String,
}

impl McpHandler {
    pub fn new(tools: Arc<dyn ToolBackend>) -> Self {
        Self {
            tools,
            resources: None,
            prompts: None,
            completions: None,
            server_name: "nexql-mcp".into(),
            server_version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    pub fn with_resources(mut self, backend: Arc<dyn ResourceBackend>) -> Self {
        self.resources = Some(backend);
        self
    }

    pub fn with_prompts(mut self, backend: Arc<dyn PromptBackend>) -> Self {
        self.prompts = Some(backend);
        self
    }

    pub fn with_completions(mut self, backend: Arc<dyn CompletionBackend>) -> Self {
        self.completions = Some(backend);
        self
    }

    pub async fn handle_json(&self, line: &str) -> Option<JsonRpcResponse> {
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
        self.handle_request(req).await
    }

    pub async fn handle_request(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        // Notifications have no id — no response.
        let id = match req.id.clone() {
            Some(id) => id,
            None => {
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
            "resources/list" => Some(self.handle_resources_list(id, req.params).await),
            "resources/read" => Some(self.handle_resources_read(id, req.params).await),
            "resources/templates/list" => Some(self.handle_resources_templates(id)),
            "prompts/list" => Some(self.handle_prompts_list(id).await),
            "prompts/get" => Some(self.handle_prompts_get(id, req.params).await),
            "completions/complete" => Some(self.handle_completions(id, req.params).await),
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

        let mut capabilities = json!({
            "tools": { "listChanged": false }
        });
        if self.resources.is_some() {
            capabilities["resources"] = json!({ "listChanged": false });
        }
        if self.prompts.is_some() {
            capabilities["prompts"] = json!({ "listChanged": false });
        }
        if self.completions.is_some() {
            capabilities["completions"] = json!({});
        }

        JsonRpcResponse::ok(
            id,
            json!({
                "protocolVersion": version,
                "capabilities": capabilities,
                "serverInfo": {
                    "name": self.server_name,
                    "version": self.server_version
                },
                "instructions": MCP_SERVER_INSTRUCTIONS
            }),
        )
    }

    async fn handle_tools_list(&self, id: Value) -> JsonRpcResponse {
        let tools = self.tools.list_tools().await;
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
        let result = self.tools.call_tool(name, args).await;
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

    async fn handle_resources_list(&self, id: Value, params: Option<Value>) -> JsonRpcResponse {
        let Some(backend) = &self.resources else {
            return JsonRpcResponse::err(
                id,
                ProtoError::METHOD_NOT_FOUND,
                "Method not found: resources/list",
            );
        };
        let cursor = params
            .as_ref()
            .and_then(|p| p.get("cursor"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        match backend.list_resources(cursor).await {
            Ok(result) => JsonRpcResponse::ok(id, result),
            Err(e) => JsonRpcResponse::err(id, e.code, e.message),
        }
    }

    async fn handle_resources_read(&self, id: Value, params: Option<Value>) -> JsonRpcResponse {
        let Some(backend) = &self.resources else {
            return JsonRpcResponse::err(
                id,
                ProtoError::METHOD_NOT_FOUND,
                "Method not found: resources/read",
            );
        };
        let Some(uri) = params
            .as_ref()
            .and_then(|p| p.get("uri"))
            .and_then(|v| v.as_str())
        else {
            return JsonRpcResponse::err(id, ProtoError::INVALID_PARAMS, "missing uri");
        };
        match backend.read_resource(uri).await {
            Ok(result) => JsonRpcResponse::ok(id, result),
            Err(e) => JsonRpcResponse::err(id, e.code, e.message),
        }
    }

    fn handle_resources_templates(&self, id: Value) -> JsonRpcResponse {
        let Some(backend) = &self.resources else {
            return JsonRpcResponse::err(
                id,
                ProtoError::METHOD_NOT_FOUND,
                "Method not found: resources/templates/list",
            );
        };
        JsonRpcResponse::ok(id, backend.list_templates())
    }

    async fn handle_prompts_list(&self, id: Value) -> JsonRpcResponse {
        let Some(backend) = &self.prompts else {
            return JsonRpcResponse::err(
                id,
                ProtoError::METHOD_NOT_FOUND,
                "Method not found: prompts/list",
            );
        };
        JsonRpcResponse::ok(id, backend.list_prompts().await)
    }

    async fn handle_prompts_get(&self, id: Value, params: Option<Value>) -> JsonRpcResponse {
        let Some(backend) = &self.prompts else {
            return JsonRpcResponse::err(
                id,
                ProtoError::METHOD_NOT_FOUND,
                "Method not found: prompts/get",
            );
        };
        let Some(params) = params else {
            return JsonRpcResponse::err(id, ProtoError::INVALID_PARAMS, "missing params");
        };
        let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
            return JsonRpcResponse::err(id, ProtoError::INVALID_PARAMS, "missing prompt name");
        };
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        match backend.get_prompt(name, arguments).await {
            Ok(result) => JsonRpcResponse::ok(id, result),
            Err(e) => JsonRpcResponse::err(id, e.code, e.message),
        }
    }

    async fn handle_completions(&self, id: Value, params: Option<Value>) -> JsonRpcResponse {
        let Some(backend) = &self.completions else {
            return JsonRpcResponse::err(
                id,
                ProtoError::METHOD_NOT_FOUND,
                "Method not found: completions/complete",
            );
        };
        let Some(params) = params else {
            return JsonRpcResponse::err(id, ProtoError::INVALID_PARAMS, "missing params");
        };
        match backend.complete(params).await {
            Ok(result) => JsonRpcResponse::ok(id, result),
            Err(e) => JsonRpcResponse::err(id, e.code, e.message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{PromptBackend, ResourceBackend, RpcFailure, ToolCallResult, ToolDescriptor};
    use async_trait::async_trait;

    struct FakeTools;

    #[async_trait]
    impl ToolBackend for FakeTools {
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

    struct EmptyResources;

    #[async_trait]
    impl ResourceBackend for EmptyResources {
        async fn list_resources(&self, cursor: Option<String>) -> Result<Value, RpcFailure> {
            if let Some(c) = cursor
                && c == "!!!bad!!!"
            {
                return Err(RpcFailure {
                    code: ProtoError::INVALID_PARAMS,
                    message: "Invalid cursor".into(),
                });
            }
            Ok(json!({ "resources": [] }))
        }

        async fn read_resource(&self, uri: &str) -> Result<Value, RpcFailure> {
            Err(RpcFailure {
                code: ProtoError::RESOURCE_NOT_FOUND,
                message: format!("Resource not found: {uri}"),
            })
        }

        fn list_templates(&self) -> Value {
            json!({
                "resourceTemplates": [{
                    "uriTemplate": "nexql://{connectionId}/{database}/object/{schema}/{name}",
                    "name": "Database object",
                    "description": "test",
                    "mimeType": "application/json"
                }]
            })
        }
    }

    struct FakePrompts;

    #[async_trait]
    impl PromptBackend for FakePrompts {
        async fn list_prompts(&self) -> Value {
            json!({
                "prompts": [
                    {"name": "health-check", "description": "a", "arguments": []},
                    {"name": "analyze-slow-queries", "description": "b", "arguments": []},
                    {"name": "explore-schema", "description": "c", "arguments": [{"name":"topic","description":"t","required":true}]},
                    {"name": "debug-blocking", "description": "d", "arguments": []},
                    {"name": "write-migration", "description": "e", "arguments": [{"name":"change","description":"c","required":true}]},
                    {"name": "optimize-table", "description": "f", "arguments": [{"name":"ref","description":"r","required":true}]},
                    {"name": "explain-this-query", "description": "g", "arguments": [{"name":"sql","description":"s","required":true}]}
                ]
            })
        }

        async fn get_prompt(&self, name: &str, arguments: Value) -> Result<Value, RpcFailure> {
            if name == "explore-schema" {
                let topic = arguments.get("topic").and_then(|v| v.as_str()).unwrap_or("");
                if topic.is_empty() {
                    return Err(RpcFailure {
                        code: ProtoError::INVALID_PARAMS,
                        message: "Missing required argument \"topic\"".into(),
                    });
                }
            }
            Ok(json!({
                "description": "d",
                "messages": [{"role":"user","content":{"type":"text","text":"ok"}}]
            }))
        }
    }

    fn full_handler() -> McpHandler {
        McpHandler::new(Arc::new(FakeTools))
            .with_resources(Arc::new(EmptyResources))
            .with_prompts(Arc::new(FakePrompts))
    }

    #[tokio::test]
    async fn initialize_returns_verbatim_instructions() {
        let handler = full_handler();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name":"t","version":"0"} }
        });
        let resp = handler
            .handle_json(&req.to_string())
            .await
            .expect("response");
        let result = resp.result.unwrap();
        assert_eq!(
            result.get("instructions").and_then(|v| v.as_str()).unwrap(),
            MCP_SERVER_INSTRUCTIONS
        );
        assert_eq!(
            result.get("protocolVersion").and_then(|v| v.as_str()),
            Some("2025-06-18")
        );
        let caps = result.get("capabilities").unwrap();
        assert!(caps.get("tools").is_some());
        assert!(caps.get("resources").is_some());
        assert!(caps.get("prompts").is_some());
        assert!(caps.get("completions").is_none());
    }

    #[tokio::test]
    async fn unknown_method_is_32601() {
        let handler = McpHandler::new(Arc::new(FakeTools));
        let req = r#"{"jsonrpc":"2.0","id":2,"method":"nope"}"#;
        let resp = handler.handle_json(req).await.unwrap();
        assert_eq!(resp.error.as_ref().unwrap().code, -32601);
    }

    #[tokio::test]
    async fn resources_list_empty() {
        let handler = full_handler();
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"resources/list"}"#;
        let resp = handler.handle_json(req).await.unwrap();
        let resources = resp.result.unwrap()["resources"].as_array().unwrap().clone();
        assert!(resources.is_empty());
    }

    #[tokio::test]
    async fn prompts_list_count() {
        let handler = full_handler();
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"prompts/list"}"#;
        let resp = handler.handle_json(req).await.unwrap();
        let prompts = resp.result.unwrap()["prompts"].as_array().unwrap().clone();
        assert_eq!(prompts.len(), 7);
    }
}
