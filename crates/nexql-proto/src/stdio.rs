// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Stdio MCP server — newline-delimited JSON-RPC.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::backend::{CompletionBackend, PromptBackend, ResourceBackend, ToolBackend};
use crate::error::ProtoError;
use crate::handler::McpHandler;

pub struct StdioServer {
    handler: McpHandler,
}

impl StdioServer {
    pub fn new(tools: Arc<dyn ToolBackend>) -> Self {
        Self {
            handler: McpHandler::new(tools),
        }
    }

    pub fn from_handler(handler: McpHandler) -> Self {
        Self { handler }
    }

    pub fn with_resources(mut self, backend: Arc<dyn ResourceBackend>) -> Self {
        self.handler = self.handler.with_resources(backend);
        self
    }

    pub fn with_prompts(mut self, backend: Arc<dyn PromptBackend>) -> Self {
        self.handler = self.handler.with_prompts(backend);
        self
    }

    pub fn with_completions(mut self, backend: Arc<dyn CompletionBackend>) -> Self {
        self.handler = self.handler.with_completions(backend);
        self
    }

    pub async fn serve<R, W>(self, reader: R, mut writer: W) -> Result<(), ProtoError>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel::<String>(32);
        self.handler.set_outbound_tx(outbound_tx);

        let mut lines = BufReader::new(reader).lines();
        loop {
            tokio::select! {
                line_res = lines.next_line() => {
                    match line_res? {
                        Some(line) => {
                            let line = line.trim();
                            if line.is_empty() {
                                continue;
                            }
                            if let Some(resp) = self.handler.handle_json(line).await {
                                let mut out = serde_json::to_string(&resp)
                                    .map_err(|e| ProtoError::Other(e.to_string()))?;
                                out.push('\n');
                                writer.write_all(out.as_bytes()).await?;
                                writer.flush().await?;
                            }
                        }
                        None => break,
                    }
                }
                outbound_msg = outbound_rx.recv() => {
                    if let Some(mut msg) = outbound_msg {
                        if !msg.ends_with('\n') {
                            msg.push('\n');
                        }
                        writer.write_all(msg.as_bytes()).await?;
                        writer.flush().await?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{ToolCallResult, ToolDescriptor};
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use std::io::Cursor;

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

    #[tokio::test]
    async fn ping_and_tools_roundtrip_on_stdio() {
        let server = StdioServer::new(Arc::new(FakeTools));
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
