// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Parse-based SQL validation via `pg_query` (libpg_query).
//!
//! Replaces the TS prefix check at `ToolExecutor.ts:218`.

use pg_query::protobuf::node::Node as NodeEnum;
use pg_query::{NodeRef, ParseResult, parse};

use crate::access::AccessMode;
use crate::error::PolicyError;
use crate::filter::{ObjectRef, PolicyFilter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlDecision {
    Allow,
    Reject,
}

/// Validate that `sql` is permitted for `mode` (single statement, AST walk — no prefix checks).
pub fn validate_write_sql(mode: AccessMode, sql: &str) -> Result<SqlDecision, PolicyError> {
    match mode {
        AccessMode::Read => validate_readonly_sql(sql),
        AccessMode::Write => validate_mode_sql::<WritePolicy>(sql),
        AccessMode::Admin => validate_mode_sql::<AdminPolicy>(sql),
    }
}

/// Validate that `sql` is a single read-only statement (SELECT / WITH-select / EXPLAIN of those).
pub fn validate_readonly_sql(sql: &str) -> Result<SqlDecision, PolicyError> {
    let result = parse_sql(sql)?;
    if let Some(decision) = reject_if_stacked(&result) {
        return Ok(decision);
    }

    enforce_dangerous_calls(&result, false)?;

    let Some(node) = root_node(&result) else {
        return Ok(SqlDecision::Reject);
    };

    if !is_readonly_root(node) {
        return Ok(SqlDecision::Reject);
    }

    for (node_ref, _depth, ctx, _) in result.protobuf.nodes() {
        use pg_query::Context;
        match ctx {
            Context::DML | Context::DDL => return Ok(SqlDecision::Reject),
            _ => {}
        }
        if is_forbidden_always(node_ref) || is_ddl_or_admin_utility(node_ref) {
            return Ok(SqlDecision::Reject);
        }
        if let NodeRef::SelectStmt(sel) = node_ref
            && sel.into_clause.is_some()
        {
            return Ok(SqlDecision::Reject);
        }
    }

    let types = result.statement_types();
    if !types
        .iter()
        .all(|t| *t == "SelectStmt" || *t == "ExplainStmt")
    {
        return Ok(SqlDecision::Reject);
    }

    if !result.dml_tables().is_empty() || !result.ddl_tables().is_empty() {
        return Ok(SqlDecision::Reject);
    }

    Ok(SqlDecision::Allow)
}

/// Reject read queries that touch schema/table refs denied by [`PolicyFilter`].
pub fn enforce_read_table_policy(filter: &PolicyFilter, sql: &str) -> Result<(), PolicyError> {
    for table in select_table_refs(sql)? {
        filter.require_table(&table.schema, &table.name)?;
    }
    Ok(())
}

/// Tables referenced by a read query (`FROM` / `JOIN`), for PII column redaction.
pub fn select_table_refs(sql: &str) -> Result<Vec<ObjectRef>, PolicyError> {
    let result = parse_sql(sql)?;
    Ok(result
        .select_tables()
        .into_iter()
        .map(|table| {
            let (schema, name) = split_table_ref(&table);
            ObjectRef::new(schema, name)
        })
        .collect())
}

fn split_table_ref(table: &str) -> (String, String) {
    match table.split_once('.') {
        Some((schema, name)) => (schema.to_owned(), name.to_owned()),
        None => ("public".to_owned(), table.to_owned()),
    }
}

struct WritePolicy;
struct AdminPolicy;

trait SqlModePolicy {
    fn allows_ddl() -> bool;
    /// `pg_terminate_backend`, `pg_reload_conf`, and `COPY` to a file or
    /// program. File-read and session-escape functions stay blocked.
    fn allows_admin_server_ops() -> bool;
    fn allows_select_into() -> bool;
    fn is_allowed_root(node: &NodeEnum) -> bool;
    fn reject_node_in_walk(node_ref: NodeRef<'_>) -> bool;
    fn allowed_statement_types(types: &[&str]) -> bool;
    fn reject_ddl_tables(result: &ParseResult) -> bool;
}

impl SqlModePolicy for WritePolicy {
    fn allows_ddl() -> bool {
        false
    }

    fn allows_admin_server_ops() -> bool {
        false
    }

    fn allows_select_into() -> bool {
        false
    }

    fn is_allowed_root(node: &NodeEnum) -> bool {
        match node {
            NodeEnum::SelectStmt(sel) => sel.into_clause.is_none(),
            NodeEnum::InsertStmt(_) | NodeEnum::UpdateStmt(_) | NodeEnum::DeleteStmt(_) => true,
            NodeEnum::ExplainStmt(explain) => explain
                .query
                .as_ref()
                .and_then(|n| n.node.as_ref())
                .is_some_and(Self::is_allowed_root),
            _ => false,
        }
    }

    fn reject_node_in_walk(node_ref: NodeRef<'_>) -> bool {
        is_forbidden_always(node_ref) || is_ddl_or_admin_utility(node_ref) || is_ddl_node(node_ref)
    }

    fn allowed_statement_types(types: &[&str]) -> bool {
        types.iter().all(|t| {
            matches!(
                *t,
                "SelectStmt" | "InsertStmt" | "UpdateStmt" | "DeleteStmt" | "ExplainStmt"
            )
        })
    }

    fn reject_ddl_tables(result: &ParseResult) -> bool {
        !result.ddl_tables().is_empty()
    }
}

impl SqlModePolicy for AdminPolicy {
    fn allows_ddl() -> bool {
        true
    }

    fn allows_admin_server_ops() -> bool {
        true
    }

    fn allows_select_into() -> bool {
        true
    }

    fn is_allowed_root(node: &NodeEnum) -> bool {
        match node {
            NodeEnum::SelectStmt(_) => true,
            NodeEnum::InsertStmt(_)
            | NodeEnum::UpdateStmt(_)
            | NodeEnum::DeleteStmt(_)
            | NodeEnum::CreateStmt(_)
            | NodeEnum::AlterTableStmt(_)
            | NodeEnum::DropStmt(_)
            | NodeEnum::IndexStmt(_)
            | NodeEnum::TruncateStmt(_)
            | NodeEnum::RenameStmt(_)
            | NodeEnum::GrantStmt(_)
            | NodeEnum::VacuumStmt(_)
            | NodeEnum::ReindexStmt(_)
            | NodeEnum::ClusterStmt(_)
            | NodeEnum::CopyStmt(_) => true,
            NodeEnum::ExplainStmt(explain) => explain
                .query
                .as_ref()
                .and_then(|n| n.node.as_ref())
                .is_some_and(Self::is_allowed_root),
            _ => false,
        }
    }

    fn reject_node_in_walk(node_ref: NodeRef<'_>) -> bool {
        is_forbidden_always(node_ref)
    }

    fn allowed_statement_types(types: &[&str]) -> bool {
        types.iter().all(|t| {
            matches!(
                *t,
                "SelectStmt"
                    | "InsertStmt"
                    | "UpdateStmt"
                    | "DeleteStmt"
                    | "ExplainStmt"
                    | "CreateStmt"
                    | "AlterTableStmt"
                    | "DropStmt"
                    | "IndexStmt"
                    | "TruncateStmt"
                    | "RenameStmt"
                    | "GrantStmt"
                    | "VacuumStmt"
                    | "ReindexStmt"
                    | "ClusterStmt"
                    | "CopyStmt"
                    | "CreateTableAsStmt"
                    | "RefreshMatViewStmt"
                    | "CommentStmt"
                    | "AlterObjectSchemaStmt"
                    | "AlterOwnerStmt"
                    | "AlterObjectDependsStmt"
                    | "SecLabelStmt"
            )
        })
    }

    fn reject_ddl_tables(_result: &ParseResult) -> bool {
        false
    }
}

fn validate_mode_sql<P: SqlModePolicy>(sql: &str) -> Result<SqlDecision, PolicyError> {
    let result = parse_sql(sql)?;
    if let Some(decision) = reject_if_stacked(&result) {
        return Ok(decision);
    }

    enforce_dangerous_calls(&result, P::allows_admin_server_ops())?;

    let Some(node) = root_node(&result) else {
        return Ok(SqlDecision::Reject);
    };

    if !P::is_allowed_root(node) {
        return Ok(SqlDecision::Reject);
    }

    for (node_ref, _depth, ctx, _) in result.protobuf.nodes() {
        use pg_query::Context;
        if !P::allows_ddl() && ctx == Context::DDL {
            return Ok(SqlDecision::Reject);
        }
        if P::reject_node_in_walk(node_ref) {
            return Ok(SqlDecision::Reject);
        }
        if let NodeRef::SelectStmt(sel) = node_ref
            && sel.into_clause.is_some()
            && !P::allows_select_into()
        {
            return Ok(SqlDecision::Reject);
        }
    }

    if !P::allowed_statement_types(&result.statement_types()) {
        return Ok(SqlDecision::Reject);
    }

    if P::reject_ddl_tables(&result) {
        return Ok(SqlDecision::Reject);
    }

    Ok(SqlDecision::Allow)
}

fn parse_sql(sql: &str) -> Result<ParseResult, PolicyError> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err(PolicyError::SqlRejected("empty SQL".into()));
    }
    parse(trimmed).map_err(|e| PolicyError::SqlParse(e.to_string()))
}

fn reject_if_stacked(result: &ParseResult) -> Option<SqlDecision> {
    if result.protobuf.stmts.len() != 1 {
        return Some(SqlDecision::Reject);
    }
    None
}

fn root_node(result: &ParseResult) -> Option<&NodeEnum> {
    result.protobuf.stmts.first()?.stmt.as_ref()?.node.as_ref()
}

fn is_readonly_root(node: &NodeEnum) -> bool {
    match node {
        NodeEnum::SelectStmt(sel) => sel.into_clause.is_none(),
        NodeEnum::ExplainStmt(explain) => {
            let Some(inner) = explain.query.as_ref().and_then(|n| n.node.as_ref()) else {
                return false;
            };
            matches!(inner, NodeEnum::SelectStmt(s) if s.into_clause.is_none())
        }
        _ => false,
    }
}

/// Reject in every access mode — arbitrary code, cluster ops, session games.
fn is_forbidden_always(node_ref: NodeRef<'_>) -> bool {
    matches!(
        node_ref,
        NodeRef::DoStmt(_)
            | NodeRef::CallStmt(_)
            | NodeRef::LoadStmt(_)
            | NodeRef::CreatedbStmt(_)
            | NodeRef::DropdbStmt(_)
            | NodeRef::ListenStmt(_)
            | NodeRef::NotifyStmt(_)
            | NodeRef::UnlistenStmt(_)
            | NodeRef::PrepareStmt(_)
            | NodeRef::ExecuteStmt(_)
            | NodeRef::DeallocateStmt(_)
            | NodeRef::DeclareCursorStmt(_)
            | NodeRef::ClosePortalStmt(_)
            | NodeRef::FetchStmt(_)
            | NodeRef::TransactionStmt(_)
            | NodeRef::VariableSetStmt(_)
            | NodeRef::LockStmt(_)
            | NodeRef::CheckPointStmt(_)
            | NodeRef::DiscardStmt(_)
    )
}

/// Admin/maintenance utilities — allowed only in Admin mode.
fn is_ddl_or_admin_utility(node_ref: NodeRef<'_>) -> bool {
    matches!(
        node_ref,
        NodeRef::CopyStmt(_)
            | NodeRef::TruncateStmt(_)
            | NodeRef::TransactionStmt(_)
            | NodeRef::VariableSetStmt(_)
            | NodeRef::CreateStmt(_)
            | NodeRef::DropStmt(_)
            | NodeRef::AlterTableStmt(_)
            | NodeRef::IndexStmt(_)
            | NodeRef::RenameStmt(_)
            | NodeRef::GrantStmt(_)
            | NodeRef::VacuumStmt(_)
            | NodeRef::ReindexStmt(_)
            | NodeRef::ClusterStmt(_)
            | NodeRef::CheckPointStmt(_)
            | NodeRef::DiscardStmt(_)
            | NodeRef::LockStmt(_)
            | NodeRef::LoadStmt(_)
            | NodeRef::CreatedbStmt(_)
            | NodeRef::DropdbStmt(_)
    )
}

/// File reads, large-object I/O, dblink, session escapes, and the XML helpers
/// that execute SQL passed as a string (`query_to_xml`, `table_to_xml`, …)
/// are rejected in every mode. Backend cancel/terminate, `pg_reload_conf`, and `COPY` to a
/// file or program are rejected unless `allow_admin_ops`.
fn enforce_dangerous_calls(result: &ParseResult, allow_admin_ops: bool) -> Result<(), PolicyError> {
    for (node_ref, _depth, _ctx, _) in result.protobuf.nodes() {
        match node_ref {
            NodeRef::FuncCall(func) => {
                if let Some(name) = function_name(func)
                    && let Some(reason) = denied_function(&name, allow_admin_ops)
                {
                    return Err(PolicyError::SqlRejected(reason));
                }
            }
            NodeRef::CopyStmt(copy) => {
                let file_or_program = copy.is_program || !copy.filename.is_empty();
                if file_or_program && !allow_admin_ops {
                    let via = if copy.is_program { "PROGRAM" } else { "file" };
                    return Err(PolicyError::SqlRejected(format!(
                        "COPY {via} is blocked outside admin mode"
                    )));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn function_name(func: &pg_query::protobuf::FuncCall) -> Option<String> {
    let last = func.funcname.last()?;
    match last.node.as_ref()? {
        NodeEnum::String(value) => Some(value.sval.clone()),
        _ => None,
    }
}

fn denied_function(name: &str, allow_admin_ops: bool) -> Option<String> {
    let name = name.to_ascii_lowercase();
    let always_blocked = name.starts_with("pg_read_file")
        || name.starts_with("pg_read_binary_file")
        || name.starts_with("pg_ls_")
        || name.starts_with("pg_stat_file")
        || name.starts_with("pg_file_")
        || name == "pg_logdir_ls"
        || name.starts_with("lo_")
        || name == "loread"
        || name == "lowrite"
        || name.starts_with("dblink")
        || name == "set_config"
        || name.starts_with("pg_sleep")
        || executes_sql_string(&name);
    if always_blocked {
        return Some(format!("function {name} is blocked in every access mode"));
    }
    let admin_only = matches!(
        name.as_str(),
        "pg_terminate_backend" | "pg_cancel_backend" | "pg_reload_conf"
    );
    if admin_only && !allow_admin_ops {
        return Some(format!("function {name} is allowed only in admin mode"));
    }
    None
}

/// `query_to_xml('select …')` and its `table_`/`schema_`/`database_`/`cursor_`
/// siblings run SQL (or dump whole relations) that this AST walk never sees,
/// so they would bypass the denylist, `deny_tables`, and PII lineage.
fn executes_sql_string(name: &str) -> bool {
    ["query", "table", "schema", "database", "cursor"]
        .iter()
        .any(|kind| name.starts_with(&format!("{kind}_to_xml")))
}

/// Why `name` is denied outside admin-only server ops, if it is.
pub fn denied_function_reason(name: &str) -> Option<String> {
    denied_function(name, false)
}

/// Parse `sql` and reject denylisted functions and `COPY` to a file or program.
/// Used for view bodies and other SQL the outer statement does not spell out.
pub fn reject_dangerous_calls(sql: &str) -> Result<(), PolicyError> {
    let result = parse_sql(sql)?;
    enforce_dangerous_calls(&result, false)
}

/// Schema-qualified or unqualified name found in a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogName {
    pub schema: Option<String>,
    pub name: String,
}

/// Function calls in `sql`, including nested ones. The schema is set only when
/// the call is written `schema.name(...)`.
pub fn routine_refs(sql: &str) -> Result<Vec<CatalogName>, PolicyError> {
    let result = parse_sql(sql)?;
    let mut out = Vec::new();
    for (node_ref, _, _, _) in result.protobuf.nodes() {
        if let NodeRef::FuncCall(func) = node_ref
            && let Some(name) = function_name(func)
        {
            let schema = if func.funcname.len() >= 2 {
                func.funcname.iter().rev().nth(1).and_then(node_string)
            } else {
                None
            };
            out.push(CatalogName { schema, name });
        }
    }
    Ok(out)
}

/// Range variables (`FROM` / `JOIN` / DML targets) referenced by `sql`.
pub fn relation_refs(sql: &str) -> Result<Vec<CatalogName>, PolicyError> {
    let result = parse_sql(sql)?;
    let mut out = Vec::new();
    for (node_ref, _, _, _) in result.protobuf.nodes() {
        if let NodeRef::RangeVar(rv) = node_ref
            && !rv.relname.is_empty()
        {
            let schema = if rv.schemaname.is_empty() {
                None
            } else {
                Some(rv.schemaname.clone())
            };
            out.push(CatalogName {
                schema,
                name: rv.relname.clone(),
            });
        }
    }
    Ok(out)
}

fn node_string(node: &pg_query::protobuf::Node) -> Option<String> {
    match node.node.as_ref()? {
        NodeEnum::String(value) => Some(value.sval.clone()),
        _ => None,
    }
}

/// A call written in a function body (`pg_read_file(` or `pg_catalog.pg_read_file(`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCall {
    pub schema: Option<String>,
    pub name: String,
}

/// Calls in `src`. Single-quoted strings and `--` comments are skipped so a
/// mention in prose is not a call. Dollar-quoted bodies are scanned: that is
/// where `LANGUAGE sql` wrappers hide `pg_read_file`.
pub fn calls_in_source(src: &str) -> Vec<SourceCall> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        if chars[i] == '\'' {
            i += 1;
            while i < chars.len() {
                if chars[i] == '\'' {
                    if i + 1 < chars.len() && chars[i + 1] == '\'' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if chars[i] == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if is_ident_start(chars[i]) {
            let start = i;
            i += 1;
            while i < chars.len() && is_ident_cont(chars[i]) {
                i += 1;
            }
            let first: String = chars[start..i].iter().collect();
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && chars[j] == '.' {
                j += 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && is_ident_start(chars[j]) {
                    let name_at = j;
                    j += 1;
                    while j < chars.len() && is_ident_cont(chars[j]) {
                        j += 1;
                    }
                    let name: String = chars[name_at..j].iter().collect();
                    let mut k = j;
                    while k < chars.len() && chars[k].is_whitespace() {
                        k += 1;
                    }
                    if k < chars.len() && chars[k] == '(' {
                        out.push(SourceCall {
                            schema: Some(first),
                            name,
                        });
                        i = k;
                        continue;
                    }
                }
            }
            if j < chars.len() && chars[j] == '(' {
                out.push(SourceCall {
                    schema: None,
                    name: first,
                });
                i = j;
                continue;
            }
            continue;
        }
        i += 1;
    }
    out
}

/// Denied function a SQL/plpgsql body invokes, if any.
pub fn source_mentions_denied_function(src: &str) -> Option<String> {
    calls_in_source(src)
        .into_iter()
        .find_map(|call| denied_function_reason(&call.name).map(|_| call.name))
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_cont(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_ddl_node(node_ref: NodeRef<'_>) -> bool {
    matches!(
        node_ref,
        NodeRef::CreateStmt(_)
            | NodeRef::DropStmt(_)
            | NodeRef::AlterTableStmt(_)
            | NodeRef::IndexStmt(_)
            | NodeRef::TruncateStmt(_)
            | NodeRef::RenameStmt(_)
            | NodeRef::GrantStmt(_)
            | NodeRef::VacuumStmt(_)
            | NodeRef::ReindexStmt(_)
            | NodeRef::ClusterStmt(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::AccessMode;

    #[test]
    fn allows_basic_select() {
        assert_eq!(
            validate_readonly_sql("SELECT 1").unwrap(),
            SqlDecision::Allow
        );
    }

    #[test]
    fn allows_with_select() {
        assert_eq!(
            validate_readonly_sql("WITH cte AS (SELECT 1) SELECT * FROM cte").unwrap(),
            SqlDecision::Allow
        );
    }

    #[test]
    fn allows_explain_select() {
        assert_eq!(
            validate_readonly_sql("EXPLAIN SELECT 1").unwrap(),
            SqlDecision::Allow
        );
    }

    #[test]
    fn rejects_delete() {
        assert_eq!(
            validate_readonly_sql("DELETE FROM t").unwrap(),
            SqlDecision::Reject
        );
    }

    #[test]
    fn rejects_stacked() {
        assert_eq!(
            validate_readonly_sql("SELECT 1; DROP TABLE t").unwrap(),
            SqlDecision::Reject
        );
    }

    #[test]
    fn rejects_cte_dml() {
        assert_eq!(
            validate_readonly_sql("WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x").unwrap(),
            SqlDecision::Reject
        );
    }

    #[test]
    fn rejects_comment_obfuscation() {
        assert_eq!(
            validate_readonly_sql("/* c */ DELETE FROM t").unwrap(),
            SqlDecision::Reject
        );
    }

    #[test]
    fn rejects_explain_delete() {
        assert_eq!(
            validate_readonly_sql("EXPLAIN DELETE FROM t").unwrap(),
            SqlDecision::Reject
        );
    }

    #[test]
    fn write_allows_dml() {
        assert_eq!(
            validate_write_sql(AccessMode::Write, "INSERT INTO t VALUES (1)").unwrap(),
            SqlDecision::Allow
        );
        assert_eq!(
            validate_write_sql(AccessMode::Write, "UPDATE t SET a = 1").unwrap(),
            SqlDecision::Allow
        );
        assert_eq!(
            validate_write_sql(AccessMode::Write, "DELETE FROM t").unwrap(),
            SqlDecision::Allow
        );
    }

    #[test]
    fn write_allows_cte_dml() {
        assert_eq!(
            validate_write_sql(
                AccessMode::Write,
                "WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x"
            )
            .unwrap(),
            SqlDecision::Allow
        );
    }

    #[test]
    fn write_rejects_ddl() {
        assert_eq!(
            validate_write_sql(AccessMode::Write, "CREATE TABLE t (id int)").unwrap(),
            SqlDecision::Reject
        );
        assert_eq!(
            validate_write_sql(AccessMode::Write, "TRUNCATE t").unwrap(),
            SqlDecision::Reject
        );
    }

    #[test]
    fn write_rejects_stacked() {
        assert_eq!(
            validate_write_sql(AccessMode::Write, "INSERT INTO t VALUES (1); DROP TABLE t")
                .unwrap(),
            SqlDecision::Reject
        );
    }

    #[test]
    fn admin_allows_ddl() {
        assert_eq!(
            validate_write_sql(AccessMode::Admin, "CREATE TABLE t (id int)").unwrap(),
            SqlDecision::Allow
        );
        assert_eq!(
            validate_write_sql(AccessMode::Admin, "VACUUM FULL").unwrap(),
            SqlDecision::Allow
        );
    }

    #[test]
    fn admin_rejects_do_block() {
        assert_eq!(
            validate_write_sql(AccessMode::Admin, "DO $$ BEGIN DELETE FROM t; END $$").unwrap(),
            SqlDecision::Reject
        );
    }

    #[test]
    fn read_delegates_to_readonly_validator() {
        assert_eq!(
            validate_write_sql(AccessMode::Read, "DELETE FROM t").unwrap(),
            SqlDecision::Reject
        );
        assert_eq!(
            validate_write_sql(AccessMode::Read, "SELECT 1").unwrap(),
            SqlDecision::Allow
        );
    }

    #[test]
    fn enforce_read_table_policy_denies_schema() {
        let filter = crate::filter::PolicyFilter {
            deny_schemas: vec!["auth".into()],
            ..Default::default()
        };
        let err = enforce_read_table_policy(&filter, "SELECT * FROM auth.credentials")
            .unwrap_err()
            .to_string();
        assert!(err.contains("denied"));
    }

    #[test]
    fn enforce_read_table_policy_allows_public_default() {
        let filter = crate::filter::PolicyFilter::default();
        enforce_read_table_policy(&filter, "SELECT * FROM users").unwrap();
    }

    #[test]
    fn read_mode_blocks_file_and_session_functions() {
        let cases = [
            "SELECT pg_read_file('/etc/passwd')",
            "SELECT pg_catalog.pg_read_file('/etc/passwd')",
            "SELECT (SELECT pg_read_file('/etc/passwd'))",
            "WITH f AS (SELECT pg_read_file('/etc/passwd') AS body) SELECT body FROM f",
            "SELECT pg_read_file('/etc/passwd') FROM LATERAL (SELECT 1) s",
            "SELECT set_config('default_transaction_read_only', 'off', false)",
            "SELECT pg_sleep(10)",
            "SELECT pg_read_binary_file('/etc/passwd')",
            "SELECT pg_ls_dir('/tmp')",
            "SELECT pg_stat_file('/etc/passwd')",
            "SELECT lo_import('/etc/passwd')",
            "SELECT lo_export(1, '/tmp/x')",
            "SELECT lo_get(1)",
            "SELECT loread(0, 10)",
            "SELECT lowrite(0, 'x')",
            "SELECT lo_from_bytea(0, 'x')",
            "SELECT dblink_connect('host=localhost')",
            "SELECT pg_terminate_backend(1)",
            "SELECT pg_cancel_backend(1)",
            "SELECT pg_reload_conf()",
            "COPY t TO '/tmp/x'",
            "COPY t FROM PROGRAM 'cat /etc/passwd'",
            "SELECT query_to_xml('select pg_read_file(''/etc/passwd'')', true, false, '')",
            "SELECT pg_catalog.query_to_xml_and_xmlschema('select 1', true, false, '')",
            "SELECT table_to_xml('customers.users', true, false, '')",
            "SELECT schema_to_xml('public', true, false, '')",
            "SELECT database_to_xml(true, false, '')",
            "SELECT cursor_to_xml('c', 10, true, false, '')",
            "SELECT pg_file_write('/tmp/x', 'y', false)",
        ];
        for sql in cases {
            let err = validate_readonly_sql(sql).expect_err(sql);
            let msg = err.to_string();
            assert!(
                msg.contains("blocked") || msg.contains("allowed only"),
                "{sql}: {msg}"
            );
        }
    }

    #[test]
    fn nested_pg_read_file_in_function_args_is_blocked() {
        let err = validate_readonly_sql("SELECT length(pg_catalog.pg_read_file('/etc/passwd'))")
            .unwrap_err();
        assert!(err.to_string().contains("pg_read_file"), "{err}");
    }

    #[test]
    fn write_mode_blocks_file_copy_and_admin_functions() {
        for sql in [
            "SELECT pg_read_file('/etc/passwd')",
            "SELECT set_config('default_transaction_read_only', 'off', false)",
            "COPY t TO '/tmp/x'",
            "COPY t FROM PROGRAM 'id'",
            "SELECT pg_terminate_backend(1)",
        ] {
            assert!(validate_write_sql(AccessMode::Write, sql).is_err(), "{sql}");
        }
    }

    #[test]
    fn admin_allows_terminate_and_file_copy_but_not_file_read() {
        assert_eq!(
            validate_write_sql(AccessMode::Admin, "SELECT pg_terminate_backend(1)").unwrap(),
            SqlDecision::Allow
        );
        assert_eq!(
            validate_write_sql(AccessMode::Admin, "COPY t TO '/tmp/x'").unwrap(),
            SqlDecision::Allow
        );
        assert_eq!(
            validate_write_sql(AccessMode::Admin, "COPY t FROM PROGRAM 'id'").unwrap(),
            SqlDecision::Allow
        );
        let err = validate_write_sql(AccessMode::Admin, "SELECT pg_read_file('/etc/passwd')")
            .unwrap_err();
        assert!(err.to_string().contains("pg_read_file"), "{err}");
        let err = validate_write_sql(
            AccessMode::Admin,
            "SELECT set_config('default_transaction_read_only', 'off', false)",
        )
        .unwrap_err();
        assert!(err.to_string().contains("set_config"), "{err}");
    }

    #[test]
    fn ordinary_functions_stay_allowed() {
        assert_eq!(
            validate_readonly_sql("SELECT lower('A')").unwrap(),
            SqlDecision::Allow
        );
        assert_eq!(
            validate_readonly_sql("SELECT 1").unwrap(),
            SqlDecision::Allow
        );
    }

    #[test]
    fn select_table_refs_collects_joined_tables() {
        let tables = select_table_refs(
            "SELECT u.id, o.total FROM public.users u JOIN public.orders o ON o.user_id = u.id",
        )
        .unwrap();
        let names: Vec<_> = tables.iter().map(|t| t.qualified()).collect();
        assert!(names.contains(&"public.users".to_string()));
        assert!(names.contains(&"public.orders".to_string()));
    }

    #[test]
    fn function_bodies_expose_denied_calls_and_skip_string_literals() {
        let src = "$$ SELECT pg_catalog.pg_read_file($1) $$";
        assert_eq!(
            source_mentions_denied_function(src).as_deref(),
            Some("pg_read_file")
        );
        assert!(
            source_mentions_denied_function("SELECT 'pg_read_file(' AS label").is_none(),
            "a name mentioned only inside a string is not a call"
        );
        assert!(source_mentions_denied_function("-- pg_read_file(\nSELECT 1").is_none());
    }
}
