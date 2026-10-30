// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! The one-time page behind `add_database` (see `nexql_tools::password_page`).
//!
//! Each request gets its own listener on 127.0.0.1 with a random port and a
//! 256-bit token in the path. The page only answers requests whose Host is
//! that loopback address (no DNS rebinding) and form posts from its own
//! origin, accepts a few attempts, and shuts down after a success or when it
//! expires.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use nexql_tools::{OpenedPage, PasswordForm, PasswordPage, SubmitPassword};
use tokio::sync::Notify;
use uuid::Uuid;

const PAGE_TTL: Duration = Duration::from_secs(600);
const MAX_ATTEMPTS: u32 = 5;

pub struct LocalPasswordPage {
    pub open_browser: bool,
}

struct PageState {
    token: String,
    port: u16,
    title: String,
    details: Vec<(String, String)>,
    submit: SubmitPassword,
    attempts: AtomicU32,
    finished: AtomicBool,
    shutdown: Notify,
}

#[async_trait]
impl PasswordPage for LocalPasswordPage {
    async fn open(&self, form: PasswordForm) -> Result<OpenedPage, String> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let url = format!("http://127.0.0.1:{port}/connect/{token}");

        let state = Arc::new(PageState {
            token,
            port,
            title: form.title,
            details: form.details,
            submit: form.submit,
            attempts: AtomicU32::new(0),
            finished: AtomicBool::new(false),
            shutdown: Notify::new(),
        });
        let app = Router::new()
            .route("/connect/{token}", get(show).post(submit))
            .with_state(state.clone());
        tokio::spawn(async move {
            let stop = async move {
                tokio::select! {
                    _ = state.shutdown.notified() => {}
                    _ = tokio::time::sleep(PAGE_TTL) => {}
                }
            };
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(stop)
                .await;
        });

        let opened_browser = self.open_browser && crate::setup::open_browser(&url).is_ok();
        Ok(OpenedPage {
            url,
            opened_browser,
            expires_in_secs: PAGE_TTL.as_secs(),
        })
    }
}

/// Constant-time token comparison.
fn token_matches(expected: &str, given: &str) -> bool {
    expected.len() == given.len()
        && expected
            .bytes()
            .zip(given.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// The request is for this page: right token, and addressed to the loopback
/// host it was opened on, so a page on another site can't reach it through a
/// rebound DNS name.
fn authorized(state: &PageState, token: &str, headers: &HeaderMap) -> bool {
    let host_ok = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| {
            h == format!("127.0.0.1:{}", state.port) || h == format!("localhost:{}", state.port)
        });
    host_ok && token_matches(&state.token, token)
}

fn same_origin(state: &PageState, headers: &HeaderMap) -> bool {
    match headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        None => true,
        Some(o) => {
            o == format!("http://127.0.0.1:{}", state.port)
                || o == format!("http://localhost:{}", state.port)
        }
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn respond(status: StatusCode, body: String) -> Response {
    let mut resp = (status, Html(body)).into_response();
    let h = resp.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'",
        ),
    );
    resp
}

const STYLE: &str = "
:root{--bg:#fafaf9;--fg:#1c1917;--muted:#57534e;--card:#fff;--line:#e7e5e4;--accent:#b4532a;--err:#b91c1c;--ok:#15803d}
@media (prefers-color-scheme:dark){:root{--bg:#0a0b0d;--fg:#f5f5f4;--muted:#a8a29e;--card:#141518;--line:#2a2b2f;--accent:#cc785c;--err:#f87171;--ok:#4ade80}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,sans-serif;display:flex;justify-content:center;padding:48px 16px}
main{width:100%;max-width:440px;background:var(--card);border:1px solid var(--line);border-radius:12px;padding:28px}
h1{font-size:20px;margin:0 0 4px}p{margin:0 0 16px;color:var(--muted)}
dl{display:grid;grid-template-columns:auto 1fr;gap:6px 16px;margin:0 0 20px;padding:14px;border:1px solid var(--line);border-radius:8px}
dt{color:var(--muted)}dd{margin:0;font-family:ui-monospace,monospace;word-break:break-all}
label{display:block;font-weight:600;margin-bottom:6px}
input{width:100%;padding:10px 12px;font:inherit;border:1px solid var(--line);border-radius:8px;background:var(--bg);color:var(--fg)}
button{margin-top:16px;width:100%;padding:11px;font:inherit;font-weight:600;border:0;border-radius:8px;background:var(--accent);color:#fff;cursor:pointer}
.err{color:var(--err);margin:0 0 16px}.ok{color:var(--ok)}small{display:block;margin-top:16px;color:var(--muted)}
";

fn page(title: &str, inner: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>{STYLE}</style></head><body><main>{inner}</main></body></html>",
        escape(title)
    )
}

fn form_page(state: &PageState, error: Option<&str>) -> String {
    let rows: String = state
        .details
        .iter()
        .map(|(k, v)| format!("<dt>{}</dt><dd>{}</dd>", escape(k), escape(v)))
        .collect();
    let error = error
        .map(|e| format!("<p class=\"err\" role=\"alert\">{}</p>", escape(e)))
        .unwrap_or_default();
    page(
        &state.title,
        &format!(
            "<h1>{}</h1>\
             <p>Your AI assistant asked NexQL to connect this database. Only continue if you asked for this and the details are right.</p>\
             <dl>{rows}</dl>{error}\
             <form method=\"post\" autocomplete=\"off\">\
             <label for=\"pw\">Password</label>\
             <input id=\"pw\" name=\"password\" type=\"password\" autocomplete=\"current-password\" autofocus>\
             <button type=\"submit\">Test and connect</button></form>\
             <small>The password goes from this page to NexQL on your computer, which stores it in your system keychain (or an encrypted file). Your assistant never sees it. Leave it empty if this database doesn't need one.</small>",
            escape(&state.title)
        ),
    )
}

fn closed_page() -> Response {
    respond(
        StatusCode::GONE,
        page(
            "Link used",
            "<h1>This link has been used</h1><p>Ask your assistant to start again if you still need to connect.</p>",
        ),
    )
}

async fn show(
    State(state): State<Arc<PageState>>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &token, &headers) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if state.finished.load(Ordering::SeqCst) {
        return closed_page();
    }
    respond(StatusCode::OK, form_page(&state, None))
}

async fn submit(
    State(state): State<Arc<PageState>>,
    Path(token): Path<String>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if !authorized(&state, &token, &headers) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !same_origin(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if state.finished.load(Ordering::SeqCst) {
        return closed_page();
    }
    let password = url::form_urlencoded::parse(body.as_bytes())
        .find(|(k, _)| k == "password")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();

    match (state.submit)(password).await {
        Ok(message) => {
            state.finished.store(true, Ordering::SeqCst);
            state.shutdown.notify_one();
            respond(
                StatusCode::OK,
                page(
                    &state.title,
                    &format!("<h1 class=\"ok\">Connected</h1><p>{}</p>", escape(&message)),
                ),
            )
        }
        Err(e) => {
            let attempts = state.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempts >= MAX_ATTEMPTS {
                state.finished.store(true, Ordering::SeqCst);
                state.shutdown.notify_one();
                return respond(
                    StatusCode::OK,
                    page(
                        &state.title,
                        &format!(
                            "<h1>Too many attempts</h1><p class=\"err\">{}</p><p>Ask your assistant to start again.</p>",
                            escape(&e)
                        ),
                    ),
                );
            }
            respond(StatusCode::OK, form_page(&state, Some(&e)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison() {
        assert!(token_matches("abc", "abc"));
        assert!(!token_matches("abc", "abd"));
        assert!(!token_matches("abc", "ab"));
    }

    #[test]
    fn details_are_escaped() {
        assert_eq!(escape("<b>\"x\"&'"), "&lt;b&gt;&quot;x&quot;&amp;&#39;");
    }
}
