//! Parse-based read-only SQL validation via `pg_query` (libpg_query).
//!
//! Replaces the TS prefix check at `ToolExecutor.ts:218`.

use pg_query::protobuf::node::Node as NodeEnum;
use pg_query::{NodeRef, parse};

use crate::error::PolicyError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlDecision {
    Allow,
    Reject,
}

/// Validate that `sql` is a single read-only statement (SELECT / WITH-select / EXPLAIN of those).
pub fn validate_readonly_sql(sql: &str) -> Result<SqlDecision, PolicyError> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err(PolicyError::SqlRejected("empty SQL".into()));
    }

    let result = parse(trimmed).map_err(|e| PolicyError::SqlParse(e.to_string()))?;

    // Stacked statements: more than one top-level RawStmt → reject.
    if result.protobuf.stmts.len() != 1 {
        return Ok(SqlDecision::Reject);
    }

    let Some(raw) = result.protobuf.stmts.first() else {
        return Ok(SqlDecision::Reject);
    };
    let Some(node) = raw.stmt.as_ref().and_then(|n| n.node.as_ref()) else {
        return Ok(SqlDecision::Reject);
    };

    if !is_readonly_root(node) {
        return Ok(SqlDecision::Reject);
    }

    // Writable CTEs / nested DML: any DML or DDL context in the walk → reject.
    // Also reject SELECT INTO (into_clause) and COPY / utility statements.
    for (node_ref, _depth, ctx, _) in result.protobuf.nodes() {
        use pg_query::Context;
        match ctx {
            Context::DML | Context::DDL => return Ok(SqlDecision::Reject),
            _ => {}
        }
        if matches!(
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
                | NodeRef::ListenStmt(_)
                | NodeRef::NotifyStmt(_)
                | NodeRef::UnlistenStmt(_)
                | NodeRef::DoStmt(_)
                | NodeRef::CallStmt(_)
                | NodeRef::PrepareStmt(_)
                | NodeRef::ExecuteStmt(_)
                | NodeRef::DeallocateStmt(_)
                | NodeRef::DeclareCursorStmt(_)
                | NodeRef::ClosePortalStmt(_)
                | NodeRef::FetchStmt(_)
                | NodeRef::ReindexStmt(_)
                | NodeRef::ClusterStmt(_)
                | NodeRef::CheckPointStmt(_)
                | NodeRef::DiscardStmt(_)
                | NodeRef::LockStmt(_)
                | NodeRef::LoadStmt(_)
                | NodeRef::CreatedbStmt(_)
                | NodeRef::DropdbStmt(_)
        ) {
            return Ok(SqlDecision::Reject);
        }
        if let NodeRef::SelectStmt(sel) = node_ref {
            if sel.into_clause.is_some() {
                return Ok(SqlDecision::Reject);
            }
        }
    }

    // Belt-and-suspenders: statement_types must be SelectStmt or ExplainStmt only.
    let types = result.statement_types();
    if !types
        .iter()
        .all(|t| *t == "SelectStmt" || *t == "ExplainStmt")
    {
        return Ok(SqlDecision::Reject);
    }

    // EXPLAIN of non-select: already rejected via is_readonly_root + walk.
    // dml_tables non-empty means writable CTE slipped through statement_types.
    if !result.dml_tables().is_empty() || !result.ddl_tables().is_empty() {
        return Ok(SqlDecision::Reject);
    }

    Ok(SqlDecision::Allow)
}

fn is_readonly_root(node: &NodeEnum) -> bool {
    match node {
        NodeEnum::SelectStmt(sel) => sel.into_clause.is_none(),
        NodeEnum::ExplainStmt(explain) => {
            let Some(inner) = explain.query.as_ref().and_then(|n| n.node.as_ref()) else {
                return false;
            };
            // EXPLAIN ANALYZE of DML is still a write — only allow EXPLAIN of SELECT.
            matches!(inner, NodeEnum::SelectStmt(s) if s.into_clause.is_none())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
