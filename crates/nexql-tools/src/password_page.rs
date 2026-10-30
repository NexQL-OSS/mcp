// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! One-time local page where the human types a database password, so
//! `add_database` can connect a database without the password ever passing
//! through the chat, the model, or a tool argument.
//!
//! `nexql-tools` has no HTTP server, so this trait is the seam: `nexql-mcp`
//! (the binary, which has axum) implements it and injects it into
//! [`crate::session::ToolSession`] for local stdio launches. When nothing is
//! injected (tests, a server the human can't reach on 127.0.0.1),
//! `add_database` falls back to a terminal command with a hidden prompt.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;

/// Called with the password the human typed. `Ok` carries a short message for
/// the page ("Connected to …"); `Err` is shown on the page so the human can
/// try again.
pub type SubmitPassword = Arc<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> + Send + Sync,
>;

/// What the page shows and what it does with the answer.
pub struct PasswordForm {
    /// Heading, e.g. `Connect "billing"`.
    pub title: String,
    /// Label / value pairs the human checks before typing anything.
    pub details: Vec<(String, String)>,
    pub submit: SubmitPassword,
}

/// A page that is open and waiting for the human.
pub struct OpenedPage {
    pub url: String,
    /// Whether a browser window was opened on the human's machine.
    pub opened_browser: bool,
    pub expires_in_secs: u64,
}

#[async_trait]
pub trait PasswordPage: Send + Sync {
    /// Serve `form` on a single-use page bound to 127.0.0.1. The page closes
    /// after one successful submit or when it expires.
    async fn open(&self, form: PasswordForm) -> Result<OpenedPage, String>;
}
