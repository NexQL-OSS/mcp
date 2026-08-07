// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Streamable HTTP MCP server — JSON-RPC over HTTP POST.
//!
//! Implements the spec's session lifecycle (`Mcp-Session-Id` issued on
//! `initialize`, required on every subsequent request, released via `DELETE`)
//! and a per-token/per-IP fixed-window rate limit.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::Value;
use uuid::Uuid;

use crate::error::ProtoError;
use crate::handler::McpHandler;
use crate::types::{JsonRpcRequest, JsonRpcResponse};

const MAX_BODY_BYTES: usize = 1024 * 1024;
const SESSION_HEADER: &str = "Mcp-Session-Id";
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);

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

/// Live Streamable HTTP sessions: issued on `initialize`, required on every
/// subsequent request, released via `DELETE`.
#[derive(Clone, Default)]
struct SessionStore {
    ids: Arc<RwLock<HashSet<String>>>,
}

impl SessionStore {
    fn issue(&self) -> String {
        let id = Uuid::new_v4().to_string();
        self.ids
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone());
        id
    }

    fn contains(&self, id: &str) -> bool {
        self.ids
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains(id)
    }

    fn remove(&self, id: &str) -> bool {
        self.ids
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id)
    }
}

/// Fixed-window rate limiter keyed by bearer token when one is configured,
/// otherwise by client IP. `max_per_window == 0` disables limiting.
#[derive(Clone)]
struct RateLimiter {
    max_per_window: u32,
    buckets: Arc<RwLock<HashMap<String, (Instant, u32)>>>,
}

impl RateLimiter {
    fn new(max_per_window: u32) -> Self {
        Self {
            max_per_window,
            buckets: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn check(&self, key: &str) -> bool {
        if self.max_per_window == 0 {
            return true;
        }
        let mut buckets = self.buckets.write().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let entry = buckets.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= RATE_LIMIT_WINDOW {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= self.max_per_window
    }
}

#[derive(Clone)]
struct AppState {
    handler: Arc<McpHandler>,
    auth: HttpAuth,
    sessions: SessionStore,
    rate_limiter: RateLimiter,
}

pub struct HttpServer {
    handler: Arc<McpHandler>,
    bind: String,
    port: u16,
    auth: HttpAuth,
    rate_limit_per_min: u32,
}

impl HttpServer {
    pub fn new(handler: McpHandler, bind: impl Into<String>, port: u16, auth: HttpAuth) -> Self {
        Self {
            handler: Arc::new(handler),
            bind: bind.into(),
            port,
            auth,
            rate_limit_per_min: 600,
        }
    }

    /// Requests per 60s window allowed per bearer token (or per client IP
    /// when no token is configured). `0` disables the limit.
    pub fn with_rate_limit_per_min(mut self, max: u32) -> Self {
        self.rate_limit_per_min = max;
        self
    }

    pub async fn serve(self) -> Result<(), ProtoError> {
        let addr: SocketAddr = format!("{}:{}", self.bind, self.port)
            .parse()
            .map_err(|e| ProtoError::Other(format!("invalid bind address: {e}")))?;

        let state = AppState {
            handler: self.handler,
            auth: self.auth,
            sessions: SessionStore::default(),
            rate_limiter: RateLimiter::new(self.rate_limit_per_min),
        };

        let app = Router::new()
            .route(
                "/",
                post(handle_mcp)
                    .get(method_not_allowed)
                    .delete(handle_delete),
            )
            .route(
                "/mcp",
                post(handle_mcp)
                    .get(method_not_allowed)
                    .delete(handle_delete),
            )
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| ProtoError::Other(format!("bind {addr}: {e}")))?;

        tracing::info!("nexql-mcp HTTP listening on http://{addr}");

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .map_err(|e| ProtoError::Other(format!("http serve: {e}")))?;

        Ok(())
    }
}

async fn method_not_allowed() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "POST, DELETE")],
        "Method Not Allowed",
    )
        .into_response()
}

fn rate_limit_key(auth: &HttpAuth, headers: &HeaderMap, client: SocketAddr) -> String {
    if let Some(token) = &auth.token {
        return token.clone();
    }
    // Fall back to whatever bearer was presented (even if auth is open),
    // then the connecting IP — keeps the loopback/no-token dev path from
    // sharing a single global bucket across every client.
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| client.ip().to_string())
}

async fn handle_mcp(
    State(state): State<AppState>,
    ConnectInfo(client): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !state.auth.check(&headers) {
        return unauthorized();
    }

    if !state
        .rate_limiter
        .check(&rate_limit_key(&state.auth, &headers, client))
    {
        return too_many_requests();
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
            let resp =
                JsonRpcResponse::err(Value::Null, ProtoError::PARSE, format!("parse error: {e}"));
            return json_response(StatusCode::BAD_REQUEST, resp);
        }
    };

    let is_initialize = req.method.as_deref() == Some("initialize");
    let session_id = if is_initialize {
        Some(state.sessions.issue())
    } else {
        match headers.get(SESSION_HEADER).and_then(|v| v.to_str().ok()) {
            Some(id) if state.sessions.contains(id) => Some(id.to_string()),
            Some(_unknown) => return session_not_found(),
            None => return session_required(),
        }
    };

    // Notifications (no id) — acknowledge without a JSON-RPC body.
    if req.id.is_none() {
        return with_session_header(StatusCode::ACCEPTED.into_response(), session_id.as_deref());
    }

    let response = state.handler.handle_request(req).await;
    let resp = match response {
        Some(resp) => json_response(StatusCode::OK, resp),
        None => StatusCode::ACCEPTED.into_response(),
    };
    with_session_header(resp, session_id.as_deref())
}

async fn handle_delete(
    State(state): State<AppState>,
    ConnectInfo(client): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !state.auth.check(&headers) {
        return unauthorized();
    }
    if !state
        .rate_limiter
        .check(&rate_limit_key(&state.auth, &headers, client))
    {
        return too_many_requests();
    }

    let Some(id) = headers.get(SESSION_HEADER).and_then(|v| v.to_str().ok()) else {
        return session_required();
    };
    if state.sessions.remove(id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        session_not_found()
    }
}

fn with_session_header(mut resp: Response, session_id: Option<&str>) -> Response {
    if let Some(id) = session_id {
        if let Ok(value) = HeaderValue::from_str(id) {
            resp.headers_mut().insert(SESSION_HEADER, value);
        }
    }
    resp
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"error":"unauthorized"}"#.to_string(),
    )
        .into_response()
}

fn too_many_requests() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"error":"rate limit exceeded"}"#.to_string(),
    )
        .into_response()
}

fn session_required() -> Response {
    (
        StatusCode::BAD_REQUEST,
        [(header::CONTENT_TYPE, "application/json")],
        format!(r#"{{"error":"missing {SESSION_HEADER} header"}}"#),
    )
        .into_response()
}

fn session_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "application/json")],
        format!(r#"{{"error":"unknown or expired {SESSION_HEADER}"}}"#),
    )
        .into_response()
}

fn json_response(status: StatusCode, payload: JsonRpcResponse) -> Response {
    match serde_json::to_string(&payload) {
        Ok(body) => (status, [(header::CONTENT_TYPE, "application/json")], body).into_response(),
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

    fn test_state(token: Option<&str>, rate_limit_per_min: u32) -> AppState {
        AppState {
            handler: Arc::new(McpHandler::new(Arc::new(FakeTools))),
            auth: HttpAuth {
                token: token.map(str::to_owned),
            },
            sessions: SessionStore::default(),
            rate_limiter: RateLimiter::new(rate_limit_per_min),
        }
    }

    fn test_app(token: Option<&str>) -> Router {
        test_app_with_rate_limit(token, 0)
    }

    fn test_app_with_rate_limit(token: Option<&str>, rate_limit_per_min: u32) -> Router {
        let state = test_state(token, rate_limit_per_min);
        Router::new()
            .route(
                "/",
                post(handle_mcp)
                    .get(method_not_allowed)
                    .delete(handle_delete),
            )
            .with_state(state)
    }

    fn conn_info() -> ConnectInfo<SocketAddr> {
        ConnectInfo("127.0.0.1:0".parse().unwrap())
    }

    /// axum's `oneshot` on a plain `Router` doesn't thread `ConnectInfo`
    /// through extensions the way a real listener (bound via
    /// `into_make_service_with_connect_info`) does, so tests insert the
    /// extension into the request directly instead.
    async fn oneshot_with_conn_info(app: Router, mut req: Request<Body>) -> Response {
        req.extensions_mut().insert(conn_info());
        app.oneshot(req).await.unwrap()
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
        let resp = oneshot_with_conn_info(app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn accepts_valid_bearer_and_issues_session_on_initialize() {
        let app = test_app(Some("secret"));
        let req = Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            ))
            .unwrap();
        let resp = oneshot_with_conn_info(app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(SESSION_HEADER).is_some());
    }

    #[tokio::test]
    async fn allows_unauthenticated_when_no_token() {
        let app = test_app(None);
        let req = Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            ))
            .unwrap();
        let resp = oneshot_with_conn_info(app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn non_initialize_without_session_header_is_rejected() {
        let app = test_app(None);
        let req = Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#))
            .unwrap();
        let resp = oneshot_with_conn_info(app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn non_initialize_with_unknown_session_header_is_not_found() {
        let app = test_app(None);
        let req = Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .header(SESSION_HEADER, "not-a-real-session")
            .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#))
            .unwrap();
        let resp = oneshot_with_conn_info(app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn session_established_then_reused_then_deleted() {
        let state = test_state(None, 0);
        let app = Router::new()
            .route(
                "/",
                post(handle_mcp)
                    .get(method_not_allowed)
                    .delete(handle_delete),
            )
            .with_state(state);

        let init_req = Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            ))
            .unwrap();
        let init_resp = oneshot_with_conn_info(app.clone(), init_req).await;
        assert_eq!(init_resp.status(), StatusCode::OK);
        let session_id = init_resp
            .headers()
            .get(SESSION_HEADER)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let ping_req = Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .header(SESSION_HEADER, session_id.clone())
            .body(Body::from(r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#))
            .unwrap();
        let ping_resp = oneshot_with_conn_info(app.clone(), ping_req).await;
        assert_eq!(ping_resp.status(), StatusCode::OK);

        let delete_req = Request::builder()
            .method("DELETE")
            .uri("/")
            .header(SESSION_HEADER, session_id.clone())
            .body(Body::empty())
            .unwrap();
        let delete_resp = oneshot_with_conn_info(app.clone(), delete_req).await;
        assert_eq!(delete_resp.status(), StatusCode::NO_CONTENT);

        let after_delete_req = Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .header(SESSION_HEADER, session_id)
            .body(Body::from(r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#))
            .unwrap();
        let after_delete_resp = oneshot_with_conn_info(app, after_delete_req).await;
        assert_eq!(after_delete_resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_without_session_header_is_bad_request() {
        let app = test_app(None);
        let req = Request::builder()
            .method("DELETE")
            .uri("/")
            .body(Body::empty())
            .unwrap();
        let resp = oneshot_with_conn_info(app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn rate_limit_blocks_after_max_per_window() {
        let app = test_app_with_rate_limit(None, 2);
        let make_req = || {
            Request::builder()
                .method("POST")
                .uri("/")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                ))
                .unwrap()
        };

        let r1 = oneshot_with_conn_info(app.clone(), make_req()).await;
        assert_eq!(r1.status(), StatusCode::OK);
        let r2 = oneshot_with_conn_info(app.clone(), make_req()).await;
        assert_eq!(r2.status(), StatusCode::OK);
        let r3 = oneshot_with_conn_info(app.clone(), make_req()).await;
        assert_eq!(r3.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}
