// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Safety policy — access modes, schema/table filters, row caps, SQL validation.

pub mod access;
pub mod caps;
pub mod error;
pub mod filter;
pub mod sql;

pub use access::{AccessMode, check_superuser_guard};
pub use caps::{
    AGENT_STATEMENT_TIMEOUT_MS, DEFAULT_MAX_RESULT_CHARS, DEFAULT_MAX_ROWS,
    DEFAULT_STATEMENT_TIMEOUT_MS, PolicyCaps, clamp_max_rows, clamp_statement_timeout_ms,
};
pub use error::PolicyError;
pub use filter::{
    ObjectRef, PolicyFilter, PII_REDACTED, column_matches_pii_policy, is_pii_column,
};
pub use sql::{
    SqlDecision, enforce_read_table_policy, select_table_refs, validate_readonly_sql,
    validate_write_sql,
};
