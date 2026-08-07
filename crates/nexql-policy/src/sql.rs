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
    fn select_table_refs_collects_joined_tables() {
        let tables = select_table_refs(
            "SELECT u.id, o.total FROM public.users u JOIN public.orders o ON o.user_id = u.id",
        )
        .unwrap();
        let names: Vec<_> = tables.iter().map(|t| t.qualified()).collect();
        assert!(names.contains(&"public.users".to_string()));
        assert!(names.contains(&"public.orders".to_string()));
    }
}
