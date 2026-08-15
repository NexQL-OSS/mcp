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
//! All six signals from the field report are implemented here: `LIMIT`
//! without `ORDER BY`, a zero-rows equality filter (SELECT and, via
//! [`dml_equality_filter`], UPDATE/DELETE), fan-out (output rows vastly
//! exceeding the largest input table), a null-skipping `AVG`/`SUM`, and a
//! sequential scan on a large table. Data-access-dependent signals (row
//! estimates, null fractions, the seq-scan EXPLAIN) are computed by the
//! caller (`exec.rs`'s `attach_critique`) and passed in or looked up via the
//! schema index — this module only does AST inspection and pure plan-JSON
//! parsing, no I/O.

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

/// Same zero-rows equality-filter extraction as [`simple_equality_filter`],
/// but for `UPDATE`/`DELETE` — DML has the same "filter matched nothing"
/// blind spot (`UPDATE orders SET ... WHERE status = 'complete'` touching 0
/// rows is just as silently-wrong as the equivalent SELECT). Returns the
/// target table too, since UPDATE/DELETE name it directly — no FROM-clause
/// scan needed.
pub fn dml_equality_filter(sql: &str) -> Option<(String, String, String)> {
    let result = pg_query::parse(sql).ok()?;
    if result.protobuf.stmts.len() != 1 {
        return None;
    }
    let node = result
        .protobuf
        .stmts
        .first()?
        .stmt
        .as_deref()?
        .node
        .as_ref()?;
    let (relation, where_clause) = match node {
        NodeEnum::UpdateStmt(u) => (u.relation.as_ref()?, u.where_clause.as_deref()),
        NodeEnum::DeleteStmt(d) => (d.relation.as_ref()?, d.where_clause.as_deref()),
        _ => return None,
    };
    let (col, val) = equality_from_node(where_clause?.node.as_ref()?)?;
    let schema = if relation.schemaname.is_empty() {
        "public"
    } else {
        relation.schemaname.as_str()
    };
    Some((format!("{schema}.{}", relation.relname), col, val))
}

/// A bare `AVG(col)`/`SUM(col)` in the target list — no `DISTINCT`, no
/// expression inside, just a column. These aggregates silently exclude NULLs
/// from the computation, which changes the denominator/result in a way
/// that's easy to not notice.
pub fn null_skipping_aggregate(sql: &str) -> Option<(String, String)> {
    let sel = top_level_select_stmt(sql)?;
    sel.target_list.iter().find_map(|t| {
        let NodeEnum::ResTarget(rt) = t.node.as_ref()? else {
            return None;
        };
        let NodeEnum::FuncCall(fc) = rt.val.as_deref()?.node.as_ref()? else {
            return None;
        };
        if fc.args.len() != 1 || fc.agg_distinct || fc.agg_star {
            return None;
        }
        let NodeEnum::String(fname) = fc.funcname.last()?.node.as_ref()? else {
            return None;
        };
        let fname_lower = fname.sval.to_ascii_lowercase();
        if fname_lower != "avg" && fname_lower != "sum" {
            return None;
        }
        let col = column_name(fc.args.first()?.node.as_ref()?)?;
        Some((fname_lower, col))
    })
}

/// `(relation_name, estimated_rows)` for every `Seq Scan` node in an
/// `EXPLAIN (FORMAT JSON)` plan (planner-only — `Plan Rows`, no `ANALYZE`
/// needed) whose estimate exceeds `threshold`. Accepts either the bare
/// `{"Plan": {...}}` root or the row-wrapped `[{"Plan": {...}, ...}]` shape
/// Postgres actually returns for `FORMAT JSON`.
pub fn large_seq_scans(query_plan: &Value, threshold: f64) -> Vec<(String, f64)> {
    let root = query_plan.get("Plan").or_else(|| {
        query_plan
            .as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.get("Plan"))
    });
    let mut out = Vec::new();
    if let Some(root) = root {
        walk_seq_scans(root, threshold, &mut out);
    }
    out
}

fn walk_seq_scans(node: &Value, threshold: f64, out: &mut Vec<(String, f64)>) {
    if node.get("Node Type").and_then(|v| v.as_str()) == Some("Seq Scan") {
        let rows = node
            .get("Plan Rows")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        if rows > threshold {
            let relation = node
                .get("Relation Name")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_owned();
            out.push((relation, rows));
        }
    }
    if let Some(children) = node.get("Plans").and_then(|v| v.as_array()) {
        for child in children {
            walk_seq_scans(child, threshold, out);
        }
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

    #[test]
    fn dml_equality_filter_extracts_update() {
        let (table, col, val) =
            dml_equality_filter("UPDATE orders SET shipped = true WHERE status = 'complete'")
                .unwrap();
        assert_eq!(table, "public.orders");
        assert_eq!(col, "status");
        assert_eq!(val, "complete");
    }

    #[test]
    fn dml_equality_filter_extracts_delete_qualified_table() {
        let (table, col, val) =
            dml_equality_filter("DELETE FROM app.orders WHERE status = 'complete'").unwrap();
        assert_eq!(table, "app.orders");
        assert_eq!(col, "status");
        assert_eq!(val, "complete");
    }

    #[test]
    fn dml_equality_filter_none_for_select() {
        assert!(dml_equality_filter("SELECT * FROM orders WHERE status = 'complete'").is_none());
    }

    #[test]
    fn null_skipping_aggregate_detects_avg() {
        let (func, col) = null_skipping_aggregate("SELECT AVG(price) FROM orders").unwrap();
        assert_eq!(func, "avg");
        assert_eq!(col, "price");
    }

    #[test]
    fn null_skipping_aggregate_detects_sum() {
        let (func, col) = null_skipping_aggregate("SELECT SUM(price) FROM orders").unwrap();
        assert_eq!(func, "sum");
        assert_eq!(col, "price");
    }

    #[test]
    fn null_skipping_aggregate_ignores_other_functions() {
        assert!(null_skipping_aggregate("SELECT COUNT(*) FROM orders").is_none());
        assert!(null_skipping_aggregate("SELECT MAX(price) FROM orders").is_none());
    }

    #[test]
    fn null_skipping_aggregate_ignores_distinct_and_expressions() {
        assert!(null_skipping_aggregate("SELECT AVG(DISTINCT price) FROM orders").is_none());
        assert!(null_skipping_aggregate("SELECT AVG(price * 2) FROM orders").is_none());
    }

    #[test]
    fn large_seq_scans_finds_relation_over_threshold() {
        let plan = json!([{
            "Plan": {
                "Node Type": "Seq Scan",
                "Relation Name": "orders",
                "Plan Rows": 4_200_000.0,
            }
        }]);
        let hits = large_seq_scans(&plan, 100_000.0);
        assert_eq!(hits, vec![("orders".to_string(), 4_200_000.0)]);
    }

    #[test]
    fn large_seq_scans_ignores_small_tables_and_descends_children() {
        let plan = json!({
            "Plan": {
                "Node Type": "Hash Join",
                "Plans": [
                    { "Node Type": "Seq Scan", "Relation Name": "small", "Plan Rows": 10.0 },
                    { "Node Type": "Seq Scan", "Relation Name": "big", "Plan Rows": 500_000.0 },
                ]
            }
        });
        let hits = large_seq_scans(&plan, 100_000.0);
        assert_eq!(hits, vec![("big".to_string(), 500_000.0)]);
    }
}
