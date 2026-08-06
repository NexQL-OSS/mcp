//! Async backends injected by the binary (avoids nexql-tools → nexql-proto).

use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub text: String,
    pub structured: Option<Value>,
    pub is_error: bool,
}

/// JSON-RPC failure with an MCP/protocol error code.
#[derive(Debug, Clone)]
pub struct RpcFailure {
    pub code: i32,
    pub message: String,
}

#[async_trait]
pub trait ToolBackend: Send + Sync {
    async fn list_tools(&self) -> Vec<ToolDescriptor>;
    async fn call_tool(&self, name: &str, arguments: Value) -> ToolCallResult;
}

#[async_trait]
pub trait ResourceBackend: Send + Sync {
    async fn list_resources(&self, cursor: Option<String>) -> Result<Value, RpcFailure>;
    async fn read_resource(&self, uri: &str) -> Result<Value, RpcFailure>;
    fn list_templates(&self) -> Value;
}

#[async_trait]
pub trait PromptBackend: Send + Sync {
    async fn list_prompts(&self) -> Value;
    async fn get_prompt(&self, name: &str, arguments: Value) -> Result<Value, RpcFailure>;
}

#[async_trait]
pub trait CompletionBackend: Send + Sync {
    async fn complete(&self, params: Value) -> Result<Value, RpcFailure>;
}

#[derive(Debug, Clone, Default)]
pub struct ClientCapabilities {
    pub elicitation: bool,
    pub roots: bool,
    pub sampling: bool,
}

#[async_trait]
pub trait ClientRequester: Send + Sync {
    fn client_capabilities(&self) -> ClientCapabilities;
    async fn request_elicitation(
        &self,
        prompt: &str,
        requested_schema: Value,
    ) -> Result<Value, String>;
    async fn request_roots(&self) -> Result<Vec<Value>, String>;
}
