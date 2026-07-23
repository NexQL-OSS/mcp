//! Async tool backend injected by the binary (avoids nexql-tools → nexql-proto).

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

#[async_trait]
pub trait ToolBackend: Send + Sync {
    async fn list_tools(&self) -> Vec<ToolDescriptor>;
    async fn call_tool(&self, name: &str, arguments: Value) -> ToolCallResult;
}
