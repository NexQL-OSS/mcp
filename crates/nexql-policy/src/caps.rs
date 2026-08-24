// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Row / result size caps.

pub const DEFAULT_MAX_ROWS: u32 = 500;
pub const DEFAULT_MAX_RESULT_CHARS: usize = 20_000;
pub const DEFAULT_STATEMENT_TIMEOUT_MS: u32 = 30_000;
pub const AGENT_STATEMENT_TIMEOUT_MS: u32 = 5_000;
pub const MIN_MAX_ROWS: u32 = 1;
pub const MAX_MAX_ROWS: u32 = 10_000;
pub const MIN_STATEMENT_TIMEOUT_MS: u32 = 100;
pub const MAX_STATEMENT_TIMEOUT_MS: u32 = 3_600_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyCaps {
    pub max_rows: u32,
    pub max_result_chars: usize,
    pub statement_timeout_ms: u32,
}

impl Default for PolicyCaps {
    fn default() -> Self {
        Self {
            max_rows: DEFAULT_MAX_ROWS,
            max_result_chars: DEFAULT_MAX_RESULT_CHARS,
            statement_timeout_ms: DEFAULT_STATEMENT_TIMEOUT_MS,
        }
    }
}

pub fn clamp_max_rows(n: u32) -> u32 {
    n.clamp(MIN_MAX_ROWS, MAX_MAX_ROWS)
}

pub fn clamp_statement_timeout_ms(ms: u32) -> u32 {
    ms.clamp(MIN_STATEMENT_TIMEOUT_MS, MAX_STATEMENT_TIMEOUT_MS)
}

/// Default ceiling for `timeout_ms` on calls that may *raise* the statement
/// timeout above a profile's configured default (`execute_sql`, `apply_ddl`)
/// — as opposed to `run_select`'s `timeout_ms`, which may only lower it
/// below `PolicyCaps::statement_timeout_ms`. 10 minutes: generous enough for
/// bulk INSERT/DDL, still bounded so a single call can't hold a connection
/// open indefinitely.
pub const DEFAULT_MAX_RAISED_TIMEOUT_MS: u32 = 600_000;

/// Ceiling for a call's raised `timeout_ms`, from `NEXQL_MCP_MAX_TIMEOUT_MS`
/// (falls back to [`DEFAULT_MAX_RAISED_TIMEOUT_MS`] if unset or unparsable),
/// itself still bounded by [`MAX_STATEMENT_TIMEOUT_MS`].
pub fn max_raised_timeout_ms() -> u32 {
    std::env::var("NEXQL_MCP_MAX_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(DEFAULT_MAX_RAISED_TIMEOUT_MS)
        .clamp(MIN_STATEMENT_TIMEOUT_MS, MAX_STATEMENT_TIMEOUT_MS)
}

/// Resolve the statement timeout to apply for a call that may raise the
/// timeout above the connection's current default — `None` means "the
/// caller passed no `timeout_ms`; leave the connection's existing timeout
/// alone" (preserves the pre-Phase-2 behavior of a fixed pool-wide default).
/// `Some` is always clamped to `max_raised`, never left unbounded.
pub fn resolve_raised_timeout_ms(requested: Option<u32>, max_raised: u32) -> Option<u32> {
    requested.map(|n| n.min(max_raised))
}

impl PolicyCaps {
    pub fn with_max_rows(mut self, n: u32) -> Self {
        self.max_rows = clamp_max_rows(n);
        self
    }

    pub fn with_statement_timeout_ms(mut self, ms: u32) -> Self {
        self.statement_timeout_ms = clamp_statement_timeout_ms(ms);
        self
    }

    /// Returns whether `text` exceeds the char cap, and a truncated view if so.
    pub fn truncate_chars<'a>(&self, text: &'a str) -> (bool, &'a str) {
        if text.len() <= self.max_result_chars {
            return (false, text);
        }
        // Truncate on char boundary.
        let mut end = self.max_result_chars;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        (true, &text[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unlike `run_select`'s `timeout_ms` (which only ever lowers below the
    /// profile default), `execute_sql`/`apply_ddl` may raise it — bounded by
    /// the server ceiling, not the profile default.
    #[test]
    fn resolve_raised_timeout_ms_allows_raising_above_profile_default() {
        let profile_default = DEFAULT_STATEMENT_TIMEOUT_MS; // 30_000
        let requested = 120_000; // well above the profile default
        let resolved = resolve_raised_timeout_ms(Some(requested), DEFAULT_MAX_RAISED_TIMEOUT_MS);
        assert_eq!(resolved, Some(requested));
        assert!(resolved.unwrap() > profile_default);
    }

    #[test]
    fn resolve_raised_timeout_ms_capped_by_server_max() {
        let resolved = resolve_raised_timeout_ms(Some(u32::MAX), DEFAULT_MAX_RAISED_TIMEOUT_MS);
        assert_eq!(resolved, Some(DEFAULT_MAX_RAISED_TIMEOUT_MS));
    }

    #[test]
    fn resolve_raised_timeout_ms_none_when_not_requested() {
        // No timeout_ms in the call → don't touch the connection's existing
        // timeout at all (preserves pre-Phase-2 behavior).
        assert_eq!(
            resolve_raised_timeout_ms(None, DEFAULT_MAX_RAISED_TIMEOUT_MS),
            None
        );
    }

    #[test]
    fn max_rows_defaults_and_clamps() {
        assert_eq!(clamp_max_rows(0), 1);
        assert_eq!(clamp_max_rows(500), 500);
        assert_eq!(clamp_max_rows(50_000), 10_000);
        assert_eq!(PolicyCaps::default().max_rows, 500);
    }

    #[test]
    fn result_char_cap() {
        let caps = PolicyCaps::default();
        let small = "hi";
        let (trunc, out) = caps.truncate_chars(small);
        assert!(!trunc);
        assert_eq!(out, "hi");

        let big = "x".repeat(25_000);
        let (trunc, out) = caps.truncate_chars(&big);
        assert!(trunc);
        assert_eq!(out.len(), 20_000);
    }
}
