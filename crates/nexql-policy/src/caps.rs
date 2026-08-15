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
