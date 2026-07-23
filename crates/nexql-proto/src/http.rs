//! Streamable HTTP MCP server — JSON-RPC over HTTP POST.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::Value;

use crate::error::ProtoError;
use crate::handler::McpHandler;
use crate::types::{JsonRpcRequest, JsonRpcResponse};

const MAX_BODY_BYTES: usize = 1024 * 1024;

/// Bearer token guard for HTTP transport.
#[derive(Clone, Debug)]
pub struct HttpAuth {
    /// When set, `Authorization: Bearer <token>` must match.
    pub token: Option<String>,
}

impl HttpAuth {
    pub fn check(&self, headers: &HeaderMap) -> bool {
        let Some(expected) = &self.token else {
            return true;
        };
        headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == format!("Bearer {expected}"))
    }
}

#[derive(Clone)]
struct AppState {
    handler: Arc<McpHandler>,
    auth: HttpAuth,
}

pub struct HttpServer {
    handler: Arc<McpHandler>,
    bind: String,
    port: u16,
    auth: HttpAuth,
}

impl HttpServer {
    pub fn new(handler: McpHandler, bind: impl Into<String>, port: u16, auth: HttpAuth) -> Self {
        Self {
            handler: Arc::new(handler),
            bind: bind.into(),
            port,
            auth,
        }
    }

    pub async fn serve(self) -> Result<(), ProtoError> {
        let addr: SocketAddr = format!("{}:{}", self.bind, self.port)
            .parse()
            .map_err(|e| ProtoError::Other(format!("invalid bind address: {e}")))?;

        let state = AppState {
            handler: self.handler,
            auth: self.auth,
        };

        let app = Router::new()
            .route("/", post(handle_mcp).get(method_not_allowed))
            .route("/mcp", post(handle_mcp).get(method_not_allowed))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| ProtoError::Other(format!("bind {addr}: {e}")))?;

        tracing::info!("nexql-mcp HTTP listening on http://{addr}");

        axum::serve(listener, app)
            .await
            .map_err(|e| ProtoError::Other(format!("http serve: {e}")))?;

        Ok(())
    }
}

async fn method_not_allowed() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "POST")],
        "Method Not Allowed",
    )
        .into_response()
}

async fn handle_mcp(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if !state.auth.check(&headers) {
        return unauthorized();
    }

    if body.len() > MAX_BODY_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error":"Payload Too Large"}"#.to_string(),
        )
            .into_response();
    }

    let req: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            let resp = JsonRpcResponse::err(
                Value::Null,
                ProtoError::PARSE,
                format!("parse error: {e}"),
            );
            return json_response(StatusCode::BAD_REQUEST, resp);
        }
    };

    // Notifications (no id) — acknowledge without a JSON-RPC body.
    if req.id.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    let response = state.handler.handle_request(req).await;
    match response {
        Some(resp) => json_response(StatusCode::OK, resp),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"error":"unauthorized"}"#.to_string(),
    )
        .into_response()
}

fn json_response(status: StatusCode, payload: JsonRpcResponse) -> Response {
    match serde_json::to_string(&payload) {
        Ok(body) => (
            status,
            [(header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "application/json")],
            format!(r#"{{"error":"serialize: {e}"}}"#),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{ToolBackend, ToolCallResult, ToolDescriptor};
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    struct FakeTools;

    #[async_trait]
    impl ToolBackend for FakeTools {
        async fn list_tools(&self) -> Vec<ToolDescriptor> {
            vec![]
        }

        async fn call_tool(&self, _name: &str, _arguments: Value) -> ToolCallResult {
            ToolCallResult {
                text: "ok".into(),
                structured: None,
                is_error: false,
            }
        }
    }

    fn test_app(token: Option<&str>) -> Router {
        let state = AppState {
            handler: Arc::new(McpHandler::new(Arc::new(FakeTools))),
            auth: HttpAuth {
                token: token.map(str::to_owned),
            },
        };
        Router::new()
            .route("/", post(handle_mcp))
            .with_state(state)
    }

    #[tokio::test]
    async fn rejects_missing_bearer_when_token_configured() {
        let app = test_app(Some("secret"));
        let req = Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn accepts_valid_bearer() {
        let app = test_app(Some("secret"));
        let req = Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn allows_unauthenticated_when_no_token() {
        let app = test_app(None);
        let req = Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
