// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Post-execution result critique (Issue 6).
//!
//! Published text-to-SQL error analyses put the large majority of failures at
//! the schema/semantic level, not syntax — the dominant failure mode is a
//! *plausible but incorrect* result, which nothing downstream catches because
//! it isn't an error. The server sees both the query and its result, which
//! puts it in a unique position to flag a few cheap, high-value signals the
//! calling model can't see on its own.
//!
//! Scoped to the two cheapest/highest-value heuristics from the field report:
//! zero rows from an equality filter (with observed-value suggestions), and
//! `LIMIT` without `ORDER BY` (a non-deterministic result set). Fan-out,
//! null-skipping aggregates, and seq-scan detection are documented follow-up,
//! not implemented here — see Issue 6 in the findings doc.

use pg_query::protobuf;
use pg_query::protobuf::node::Node as NodeEnum;
use serde_json::{Value, json};

/// One critique finding attached to a `run_select` response's `critique` array.
#[derive(Debug, Clone)]
pub struct CritiqueItem {
    pub signal: &'static str,
    pub message: String,
}

impl CritiqueItem {
    pub fn to_json(&self) -> Value {
        json!({ "signal": self.signal, "message": self.message })
    }
}

/// `LIMIT` with no `ORDER BY`: which rows come back is arbitrary and can
/// change between identical runs — the cheapest of these signals to compute
/// (pure AST shape, no data access) and a common source of "why did this
/// change" confusion.
pub fn limit_without_order_by(sql: &str) -> Option<CritiqueItem> {
    let sel = top_level_select_stmt(sql)?;
    if sel.limit_count.is_some() && sel.sort_clause.is_empty() {
        Some(CritiqueItem {
            signal: "limit_without_order_by",
            message: "LIMIT with no ORDER BY — result set is non-deterministic; add ORDER BY if you need stable rows.".into(),
        })
    } else {
        None
    }
}

/// Extract a single top-level `WHERE col = 'literal'` equality filter (plain
/// column ref, plain literal — no functions, casts, or subqueries) for the
/// zero-rows heuristic. Descends into `AND`-ed conditions looking for the
/// first such equality; `OR` branches are left alone since emptiness doesn't
/// pin the blame on any one side.
pub fn simple_equality_filter(sql: &str) -> Option<(String, String)> {
    let sel = top_level_select_stmt(sql)?;
    let where_node = sel.where_clause.as_deref()?.node.as_ref()?;
    equality_from_node(where_node)
}

fn equality_from_node(node: &NodeEnum) -> Option<(String, String)> {
    match node {
        NodeEnum::AExpr(expr) if expr.kind == protobuf::AExprKind::AexprOp as i32 => {
            let op_node = expr.name.first()?.node.as_ref()?;
            let NodeEnum::String(op) = op_node else {
                return None;
            };
            if op.sval != "=" {
                return None;
            }
            let col = column_name(expr.lexpr.as_deref()?.node.as_ref()?)?;
            let val = literal_value(expr.rexpr.as_deref()?.node.as_ref()?)?;
            Some((col, val))
        }
        NodeEnum::BoolExpr(bexpr) if bexpr.boolop == protobuf::BoolExprType::AndExpr as i32 => {
            bexpr
                .args
                .iter()
                .find_map(|n| n.node.as_ref().and_then(equality_from_node))
        }
        _ => None,
    }
}

fn column_name(node: &NodeEnum) -> Option<String> {
    let NodeEnum::ColumnRef(col_ref) = node else {
        return None;
    };
    match col_ref.fields.last()?.node.as_ref()? {
        NodeEnum::String(s) => Some(s.sval.clone()),
        _ => None,
    }
}

fn literal_value(node: &NodeEnum) -> Option<String> {
    let NodeEnum::AConst(c) = node else {
        return None;
    };
    match c.val.as_ref()? {
        protobuf::a_const::Val::Sval(s) => Some(s.sval.clone()),
        protobuf::a_const::Val::Ival(i) => Some(i.ival.to_string()),
        protobuf::a_const::Val::Fval(f) => Some(f.fval.clone()),
        protobuf::a_const::Val::Boolval(b) => Some(b.boolval.to_string()),
        _ => None,
    }
}

/// First statement's top-level `SelectStmt` node. CTEs (`WITH ...`) attach
/// `with_clause` onto this same node, so `limit_count`/`sort_clause`/
/// `where_clause` here are always the outermost query's — no separate
/// unwrapping needed. Returns `None` for anything else (multi-statement,
/// EXPLAIN, non-SELECT) — callers treat that as "no finding", not an error.
fn top_level_select_stmt(sql: &str) -> Option<protobuf::SelectStmt> {
    let result = pg_query::parse(sql).ok()?;
    if result.protobuf.stmts.len() != 1 {
        return None;
    }
    let node = result.protobuf.stmts.first()?.stmt.as_deref()?;
    match node.node.as_ref()? {
        NodeEnum::SelectStmt(sel) => Some((**sel).clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_without_order_by_fires() {
        let item = limit_without_order_by("SELECT * FROM orders LIMIT 10").unwrap();
        assert_eq!(item.signal, "limit_without_order_by");
    }

    #[test]
    fn limit_with_order_by_is_silent() {
        assert!(limit_without_order_by("SELECT * FROM orders ORDER BY id LIMIT 10").is_none());
    }

    #[test]
    fn no_limit_is_silent() {
        assert!(limit_without_order_by("SELECT * FROM orders").is_none());
    }

    #[test]
    fn simple_equality_filter_extracts_column_and_literal() {
        let (col, val) =
            simple_equality_filter("SELECT * FROM orders WHERE status = 'complete'").unwrap();
        assert_eq!(col, "status");
        assert_eq!(val, "complete");
    }

    #[test]
    fn simple_equality_filter_descends_and() {
        let (col, val) = simple_equality_filter(
            "SELECT * FROM orders WHERE customer_id = 1 AND status = 'complete'",
        )
        .unwrap();
        assert_eq!(col, "customer_id");
        assert_eq!(val, "1");
    }

    #[test]
    fn simple_equality_filter_ignores_function_calls() {
        assert!(
            simple_equality_filter("SELECT * FROM orders WHERE lower(status) = 'complete'")
                .is_none()
        );
    }

    #[test]
    fn simple_equality_filter_none_without_where() {
        assert!(simple_equality_filter("SELECT * FROM orders").is_none());
    }

    #[test]
    fn simple_equality_filter_qualified_column() {
        let (col, val) =
            simple_equality_filter("SELECT * FROM orders o WHERE o.status = 'complete'").unwrap();
        assert_eq!(col, "status");
        assert_eq!(val, "complete");
    }
}
