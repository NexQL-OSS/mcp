// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Real human-in-the-loop confirmation via MCP `elicitation/create` — the
//! protocol's mechanism for a server to ask the *client* to collect an
//! answer from the human mid-call, as opposed to an agent-settable argument
//! (`confirm_destructive`) that an agent could set on itself without ever
//! asking anyone.
//!
//! `nexql-tools` must never depend on `nexql-proto` (see `lib.rs`), so this
//! trait is the seam: `nexql-mcp` (the binary) implements it by wrapping
//! `nexql_proto::McpHandler`'s `ClientRequester` and injects it into
//! [`crate::session::ToolSession`] at startup. When no backend is injected,
//! or the connected client didn't advertise elicitation support, callers
//! fall back to requiring an explicit confirm argument — see
//! `write::apply_ddl`'s CRITICAL-risk gate for the concrete usage.

use async_trait::async_trait;

/// Result of asking the client to elicit a yes/no confirmation from the
/// human. Distinguishes "the human said no" from "we couldn't ask" so
/// callers can react differently (a decline is a clear, final answer; a
/// failure to elicit should fall back to the static confirm-argument gate,
/// not be treated as either a yes or a no).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationOutcome {
    /// The human confirmed (MCP elicitation `action: "accept"`).
    Confirmed,
    /// The human explicitly declined or dismissed the prompt (`"decline"` /
    /// `"cancel"`).
    Declined,
}

#[async_trait]
pub trait ElicitationRequester: Send + Sync {
    /// Whether the connected client advertised `elicitation` support during
    /// `initialize` — calling `request_confirmation` when this is `false`
    /// will fail (no client-side handler to answer it).
    fn supports_elicitation(&self) -> bool;

    /// Ask the client to elicit a yes/no confirmation from the human for
    /// `prompt`. `Err` means the request itself failed (no live transport,
    /// client error, timeout) — callers must not treat that as either a
    /// confirmation or a decline.
    async fn request_confirmation(&self, prompt: &str) -> Result<ConfirmationOutcome, String>;
}
