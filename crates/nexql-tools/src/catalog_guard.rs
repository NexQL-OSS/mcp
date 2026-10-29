// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Resolve function calls against `pg_proc` before they run.
//!
//! The AST denylist sees the name that was written. A user function
//! `wrap(text)` whose body is `SELECT pg_read_file($1)` does not. This walks
//! those bodies (and `SECURITY DEFINER` functions outside `pg_catalog`) and
//! refuses the statement. See `SECURITY.md` for what a body scan cannot see.

use std::collections::HashSet;

use nexql_policy::sql::{
    calls_in_source, denied_function_reason, routine_refs, source_mentions_denied_function,
};

use crate::error::ToolError;

const MAX_DEPTH: u32 = 6;

const LOOKUP_FUNCS: &str = "\
SELECT n.nspname::text, p.prosecdef, l.lanname::text, p.prosrc
FROM pg_proc p
JOIN pg_namespace n ON n.oid = p.pronamespace
JOIN pg_language l ON l.oid = p.prolang
WHERE p.proname = $1
  AND (
    ($2::text <> '' AND n.nspname = $2)
    OR ($2::text = '' AND n.nspname = ANY (current_schemas(true)))
  )";

/// Reject calls that the AST denylist cannot see: non-`pg_catalog` functions
/// that are `SECURITY DEFINER`, or whose body (followed a few calls deep)
/// invokes a denied function.
pub async fn enforce_routine_guards(
    client: &tokio_postgres::Client,
    sql: &str,
) -> Result<(), ToolError> {
    let refs = routine_refs(sql).map_err(|err| ToolError::Execution(err.to_string()))?;
    let mut queue: Vec<(Option<String>, String, u32)> = refs
        .into_iter()
        .map(|call| (call.schema, call.name, 0))
        .collect();
    let mut seen = HashSet::new();
    while let Some((schema, name, depth)) = queue.pop() {
        if depth > MAX_DEPTH {
            return Err(ToolError::Execution(
                "refusing to run SQL: function-call expansion exceeded the safety depth, so a \
                 wrapper around a blocked function could not be ruled out"
                    .into(),
            ));
        }
        let schema_param = schema.unwrap_or_default();
        let key = format!(
            "{}.{}",
            schema_param.to_ascii_lowercase(),
            name.to_ascii_lowercase()
        );
        if !seen.insert(key) {
            continue;
        }
        if let Some(reason) = denied_function_reason(&name) {
            return Err(ToolError::Execution(reason));
        }
        let rows = client
            .query(LOOKUP_FUNCS, &[&name, &schema_param])
            .await
            .map_err(|err| {
                ToolError::Execution(format!(
                    "could not resolve function {name} against pg_proc ({err}); refusing"
                ))
            })?;
        for row in rows {
            let nsp: String = row.get(0);
            let secdef: bool = row.get(1);
            let lang: String = row.get(2);
            let src: String = row.get(3);
            if nsp.eq_ignore_ascii_case("pg_catalog") {
                continue;
            }
            let qualified = format!("{nsp}.{name}");
            if secdef {
                return Err(ToolError::Execution(format!(
                    "function {qualified} is SECURITY DEFINER and is blocked outside pg_catalog"
                )));
            }
            if let Some(hit) = source_mentions_denied_function(&src) {
                return Err(ToolError::Execution(format!(
                    "function {qualified} calls blocked function {hit}"
                )));
            }
            let lang = lang.to_ascii_lowercase();
            if lang == "c" || lang == "internal" {
                if denied_function_reason(src.trim()).is_some() {
                    return Err(ToolError::Execution(format!(
                        "function {qualified} is a {lang} wrapper around a blocked function"
                    )));
                }
                // A C symbol that is not itself a denied name cannot be inspected.
                // That residual is documented in SECURITY.md.
                continue;
            }
            for nested in calls_in_source(&src) {
                queue.push((nested.schema, nested.name, depth + 1));
            }
        }
    }
    Ok(())
}
