// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Output-column lineage for PII redaction.
//!
//! Name-only matching misses `email AS contact`, `lower(email)`, and
//! whole-row constructors (`row_to_json(u)`). This walk taints an output
//! expression when any input it reads is a configured PII column or a
//! sensitive column name, and rejects whole-row constructs that would
//! embed those columns inside one opaque value.

use std::collections::HashMap;

use pg_query::parse;
use pg_query::protobuf::node::Node as NodeEnum;
use pg_query::protobuf::{
    AExprKind, CaseExpr, CaseWhen, ColumnRef, CommonTableExpr, FuncCall, JsonExprOp, MinMaxOp,
    Node, RangeFunction, RangeSubselect, RangeVar, ResTarget, RowExpr, SelectStmt, SubLink,
    SubLinkType, XmlExprOp,
};

use crate::error::PolicyError;
use crate::filter::{PolicyFilter, configured_pii_names, is_pii_column, is_sensitive_column_name};

/// A column the caller already knows (from config, a prior catalog lookup,
/// or `RowDescription` metadata).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogColumn {
    pub schema: String,
    pub table: String,
    pub column: String,
}

/// A base relation whose column list was not available, so a whole-row or
/// star reference could not be classified. Callers load `pg_attribute` and
/// re-analyze; if it is still unresolved the query fails closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedRelation {
    pub schema: String,
    pub name: String,
}

/// What to redact or refuse in a statement's output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLineage {
    /// Result column names (aliases included) whose cells must be redacted.
    pub redact_names: Vec<String>,
    /// Whole-row constructs that cannot be redacted safely.
    pub rejections: Vec<String>,
    /// Relations that need a catalog column list before a decision is safe.
    pub unresolved_whole_rows: Vec<UnresolvedRelation>,
    /// Unqualified relation names (lowercased as parsed) that were not in
    /// `schema_of`. Callers resolve them through the session `search_path`
    /// and re-analyze; until then they are assumed to live in `public`.
    pub unqualified_relations: Vec<String>,
}

impl OutputLineage {
    pub fn is_clean(&self) -> bool {
        self.redact_names.is_empty()
            && self.rejections.is_empty()
            && self.unresolved_whole_rows.is_empty()
    }
}

/// Analyze `sql`. `known_columns` may be empty; configured PII still taints
/// matching references, and star / whole-row reads of an unknown table are
/// reported in [`OutputLineage::unresolved_whole_rows`] when sensitive-name
/// heuristics are on. Unqualified relations are assumed to be in `public`;
/// use [`analyze_output_lineage_resolved`] once their schemas are known.
pub fn analyze_output_lineage(
    sql: &str,
    filter: &PolicyFilter,
    known_columns: &[CatalogColumn],
) -> Result<OutputLineage, PolicyError> {
    analyze_output_lineage_resolved(sql, filter, known_columns, &HashMap::new())
}

/// Like [`analyze_output_lineage`], with `schema_of` mapping an unqualified
/// relation name to the schema the session `search_path` resolves it to.
pub fn analyze_output_lineage_resolved(
    sql: &str,
    filter: &PolicyFilter,
    known_columns: &[CatalogColumn],
    schema_of: &HashMap<String, String>,
) -> Result<OutputLineage, PolicyError> {
    let parsed = parse(sql.trim()).map_err(|e| PolicyError::SqlParse(e.to_string()))?;
    let mut analysis = Analysis {
        filter,
        known: known_columns,
        schema_of,
        ctes: HashMap::new(),
        redact: Vec::new(),
        rejections: Vec::new(),
        unresolved: Vec::new(),
        unqualified: Vec::new(),
    };
    for stmt in &parsed.protobuf.stmts {
        let Some(root) = stmt.stmt.as_ref().and_then(|n| n.node.as_ref()) else {
            continue;
        };
        match root {
            NodeEnum::SelectStmt(sel) => {
                analysis.analyze_select(sel, &[], true);
            }
            NodeEnum::ExplainStmt(explain) => {
                // EXPLAIN returns a plan, not the query's columns, so there is
                // nothing to redact by name; whole-row rejections still apply.
                if let Some(NodeEnum::SelectStmt(sel)) =
                    explain.query.as_ref().and_then(|n| n.node.as_ref())
                {
                    analysis.analyze_select(sel, &[], false);
                }
            }
            _ => {}
        }
    }
    analysis.redact.sort();
    analysis.redact.dedup();
    Ok(OutputLineage {
        redact_names: analysis.redact,
        rejections: analysis.rejections,
        unresolved_whole_rows: analysis.unresolved,
        unqualified_relations: analysis.unqualified,
    })
}

#[derive(Clone)]
struct DerivedCol {
    name: String,
    tainted: bool,
}

#[derive(Clone)]
enum RelKind {
    Table,
    Derived(Vec<DerivedCol>),
    /// A FROM item whose column list we do not model: set-returning
    /// functions (`jsonb_each_text(to_jsonb(u))`), `XMLTABLE`, `JSON_TABLE`.
    /// `tainted` means its inputs read sensitive data, so every column it
    /// produces is treated as sensitive.
    Opaque {
        tainted: bool,
    },
}

#[derive(Clone)]
struct Rel {
    schema: String,
    name: String,
    alias: String,
    kind: RelKind,
}

enum ExprKind {
    Value,
    Tainted,
    Star(Vec<usize>),
    WholeRow(usize),
}

enum Sensitivity {
    Clean,
    Names(Vec<String>),
    /// `also` are configured PII names we can already redact; the rest of
    /// the column list is unknown and heuristics may still apply.
    Unresolved {
        also: Vec<String>,
    },
}

struct Analysis<'a> {
    filter: &'a PolicyFilter,
    known: &'a [CatalogColumn],
    schema_of: &'a HashMap<String, String>,
    ctes: HashMap<String, Vec<DerivedCol>>,
    redact: Vec<String>,
    rejections: Vec<String>,
    unresolved: Vec<UnresolvedRelation>,
    unqualified: Vec<String>,
}

/// Label used in rejections for an opaque FROM item fed by sensitive data.
const OPAQUE_SENSITIVE_COLUMNS: &str = "all columns (computed from sensitive input)";

const WHOLE_ROW_FUNCS: &[&str] = &[
    "row_to_json",
    "to_json",
    "to_jsonb",
    "json_agg",
    "jsonb_agg",
    "array_agg",
];

impl Analysis<'_> {
    fn analyze_select(&mut self, sel: &SelectStmt, outer: &[Rel], export: bool) -> Vec<DerivedCol> {
        let saved = self.ctes.clone();
        if let Some(with) = &sel.with_clause {
            for cte in &with.ctes {
                self.define_cte(cte, with.recursive);
            }
        }
        let cols = if is_set_operation(sel.op) {
            let left = sel
                .larg
                .as_ref()
                .map(|s| self.analyze_select(s, outer, false))
                .unwrap_or_default();
            let right = sel
                .rarg
                .as_ref()
                .map(|s| self.analyze_select(s, outer, false))
                .unwrap_or_default();
            merge_union(left, right)
        } else {
            let mut scope = outer.to_vec();
            self.bind_from(&sel.from_clause, &mut scope);
            let mut cols = Vec::new();
            for target in &sel.target_list {
                let Some(NodeEnum::ResTarget(rt)) = target.node.as_ref() else {
                    continue;
                };
                cols.extend(self.analyze_target(rt, &scope, export));
            }
            cols
        };
        if is_set_operation(sel.op) && export {
            for col in &cols {
                if col.tainted {
                    self.push_redact(&col.name);
                }
            }
        }
        self.ctes = saved;
        cols
    }

    fn define_cte(&mut self, node: &Node, recursive: bool) {
        let Some(NodeEnum::CommonTableExpr(cte)) = node.node.as_ref() else {
            return;
        };
        let Some(query) = cte.ctequery.as_ref() else {
            return;
        };
        let Some(NodeEnum::SelectStmt(sel)) = query.node.as_ref() else {
            return;
        };
        let key = cte.ctename.to_ascii_lowercase();
        // Provisional entry so a recursive arm resolves as a CTE, not a table.
        self.ctes.insert(key.clone(), Vec::new());
        let mut cols = apply_cte_aliases(cte, self.analyze_select(sel, &[], false));
        if recursive {
            // Second pass so taint from the first pass flows through the
            // self-reference. Only WITH RECURSIVE pays for it; analyzing every
            // CTE twice doubled the cost at each nesting level.
            self.ctes.insert(key.clone(), cols);
            cols = apply_cte_aliases(cte, self.analyze_select(sel, &[], false));
        }
        self.ctes.insert(key, cols);
    }

    fn bind_from(&mut self, nodes: &[Node], scope: &mut Vec<Rel>) {
        for node in nodes {
            self.bind_from_node(node, scope);
        }
    }

    fn bind_from_node(&mut self, node: &Node, scope: &mut Vec<Rel>) {
        let Some(inner) = node.node.as_ref() else {
            return;
        };
        match inner {
            NodeEnum::RangeVar(rv) => scope.push(self.rel_from_range(rv)),
            NodeEnum::JoinExpr(join) => {
                if let Some(larg) = &join.larg {
                    self.bind_from_node(larg, scope);
                }
                if let Some(rarg) = &join.rarg {
                    self.bind_from_node(rarg, scope);
                }
            }
            NodeEnum::RangeSubselect(sub) => self.bind_subselect(sub, scope),
            NodeEnum::RangeTableSample(sample) => {
                if let Some(relation) = &sample.relation {
                    self.bind_from_node(relation, scope);
                }
            }
            NodeEnum::RangeFunction(func) => self.bind_function(func, scope),
            // XMLTABLE, JSON_TABLE, …: columns are not modelled, so fail
            // closed and treat everything they produce as sensitive.
            _ => scope.push(Rel {
                schema: String::new(),
                name: String::new(),
                alias: String::new(),
                kind: RelKind::Opaque { tainted: true },
            }),
        }
    }

    /// Functions in FROM may read earlier FROM items without `LATERAL`, so
    /// they are evaluated against the scope bound so far. A whole-row
    /// argument such as `to_jsonb(u)` is rejected by the usual whole-row
    /// rules; any other sensitive input taints every output column.
    fn bind_function(&mut self, func: &RangeFunction, scope: &mut Vec<Rel>) {
        let mut tainted = false;
        let mut first_name = None;
        for item in &func.functions {
            let call = match item.node.as_ref() {
                Some(NodeEnum::List(list)) => list.items.first(),
                Some(_) => Some(item),
                None => None,
            };
            let Some(call) = call else {
                continue;
            };
            if first_name.is_none()
                && let Some(NodeEnum::FuncCall(f)) = call.node.as_ref()
            {
                first_name = func_name(f);
            }
            let kind = self.eval_expr(call, scope);
            if self.kind_taints(kind, scope) {
                tainted = true;
            }
        }
        let alias = func
            .alias
            .as_ref()
            .map(|a| a.aliasname.clone())
            .filter(|s| !s.is_empty())
            .or(first_name)
            .unwrap_or_default();
        scope.push(Rel {
            schema: String::new(),
            name: alias.clone(),
            alias,
            kind: RelKind::Opaque { tainted },
        });
    }

    fn bind_subselect(&mut self, sub: &RangeSubselect, scope: &mut Vec<Rel>) {
        let outer: Vec<Rel> = if sub.lateral {
            scope.clone()
        } else {
            Vec::new()
        };
        let Some(query) = sub.subquery.as_ref() else {
            return;
        };
        let Some(NodeEnum::SelectStmt(sel)) = query.node.as_ref() else {
            return;
        };
        let mut cols = self.analyze_select(sel, &outer, false);
        if let Some(alias) = &sub.alias {
            apply_alias_colnames(&alias.colnames, &mut cols);
        }
        let alias = sub
            .alias
            .as_ref()
            .map(|a| a.aliasname.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "subquery".to_owned());
        scope.push(Rel {
            schema: String::new(),
            name: alias.clone(),
            alias,
            kind: RelKind::Derived(cols),
        });
    }

    fn rel_from_range(&mut self, rv: &RangeVar) -> Rel {
        let alias = rv
            .alias
            .as_ref()
            .map(|a| a.aliasname.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| rv.relname.clone());
        if rv.schemaname.is_empty()
            && let Some(cols) = self.ctes.get(&rv.relname.to_ascii_lowercase())
        {
            return Rel {
                schema: String::new(),
                name: rv.relname.clone(),
                alias,
                kind: RelKind::Derived(cols.clone()),
            };
        }
        let schema = if !rv.schemaname.is_empty() {
            rv.schemaname.clone()
        } else if let Some(schema) = self.schema_of.get(&rv.relname) {
            schema.clone()
        } else {
            // Caller resolves this through search_path and re-analyzes.
            if !self.unqualified.contains(&rv.relname) {
                self.unqualified.push(rv.relname.clone());
            }
            "public".to_owned()
        };
        Rel {
            schema,
            name: rv.relname.clone(),
            alias,
            kind: RelKind::Table,
        }
    }

    fn analyze_target(&mut self, rt: &ResTarget, scope: &[Rel], export: bool) -> Vec<DerivedCol> {
        let Some(val) = rt.val.as_ref() else {
            return Vec::new();
        };
        if let ExprKind::Star(idxs) = self.eval_expr(val, scope) {
            let mut cols = Vec::new();
            for idx in idxs {
                if let Some(rel) = scope.get(idx) {
                    cols.extend(self.star_cols(rel));
                }
            }
            if export {
                for col in &cols {
                    if col.tainted {
                        self.push_redact(&col.name);
                    }
                }
            }
            return cols;
        }
        let kind = self.eval_expr(val, scope);
        let tainted = self.kind_taints(kind, scope);
        let name = if rt.name.is_empty() {
            expr_name(val)
        } else {
            rt.name.clone()
        };
        if export && tainted {
            self.push_redact(&name);
        }
        vec![DerivedCol { name, tainted }]
    }

    fn kind_taints(&mut self, kind: ExprKind, scope: &[Rel]) -> bool {
        match kind {
            ExprKind::Value => false,
            ExprKind::Tainted => true,
            ExprKind::Star(idxs) => {
                let mut leaks = false;
                for idx in idxs {
                    if let Some(rel) = scope.get(idx)
                        && self.leaks_whole_row(rel)
                    {
                        self.note_whole_row(rel);
                        leaks = true;
                    }
                }
                leaks
            }
            ExprKind::WholeRow(idx) => scope.get(idx).is_some_and(|rel| {
                let leaks = self.leaks_whole_row(rel);
                if leaks {
                    self.note_whole_row(rel);
                }
                leaks
            }),
        }
    }

    fn eval_expr(&mut self, node: &Node, scope: &[Rel]) -> ExprKind {
        let Some(inner) = node.node.as_ref() else {
            return ExprKind::Value;
        };
        match inner {
            NodeEnum::ColumnRef(col) => self.eval_column(col, scope),
            NodeEnum::FuncCall(func) => self.eval_func(func, scope),
            NodeEnum::AExpr(expr) => {
                let left = expr
                    .lexpr
                    .as_ref()
                    .map(|n| self.eval_expr(n, scope))
                    .unwrap_or(ExprKind::Value);
                let right = expr
                    .rexpr
                    .as_ref()
                    .map(|n| self.eval_expr(n, scope))
                    .unwrap_or(ExprKind::Value);
                self.fold_kinds([left, right], scope)
            }
            NodeEnum::BoolExpr(expr) => {
                let kinds = expr
                    .args
                    .iter()
                    .map(|n| self.eval_expr(n, scope))
                    .collect::<Vec<_>>();
                self.fold_kinds(kinds, scope)
            }
            NodeEnum::TypeCast(cast) => cast
                .arg
                .as_ref()
                .map(|n| self.eval_expr(n, scope))
                .unwrap_or(ExprKind::Value),
            NodeEnum::CollateClause(col) => col
                .arg
                .as_ref()
                .map(|n| self.eval_expr(n, scope))
                .unwrap_or(ExprKind::Value),
            NodeEnum::CoalesceExpr(expr) => {
                let kinds = expr
                    .args
                    .iter()
                    .map(|n| self.eval_expr(n, scope))
                    .collect::<Vec<_>>();
                self.fold_kinds(kinds, scope)
            }
            NodeEnum::NullTest(test) => test
                .arg
                .as_ref()
                .map(|n| self.eval_expr(n, scope))
                .unwrap_or(ExprKind::Value),
            NodeEnum::BooleanTest(test) => test
                .arg
                .as_ref()
                .map(|n| self.eval_expr(n, scope))
                .unwrap_or(ExprKind::Value),
            NodeEnum::CaseExpr(expr) => self.eval_case(expr, scope),
            NodeEnum::CaseWhen(when) => self.eval_when(when, scope),
            NodeEnum::RowExpr(row) => self.eval_row(row, scope),
            NodeEnum::SubLink(sub) => self.eval_sublink(sub, scope),
            NodeEnum::AIndirection(ind) => {
                let base = ind
                    .arg
                    .as_ref()
                    .map(|n| self.eval_expr(n, scope))
                    .unwrap_or(ExprKind::Value);
                if let ExprKind::WholeRow(idx) = base
                    && let Some(field) = ind.indirection.iter().find_map(node_str)
                    && let Some(rel) = scope.get(idx)
                {
                    return self.sensitivity_of_column(rel, field);
                }
                let extra = ind
                    .indirection
                    .iter()
                    .map(|n| self.eval_expr(n, scope))
                    .collect::<Vec<_>>();
                self.fold_kinds(std::iter::once(base).chain(extra), scope)
            }
            NodeEnum::AIndices(idx) => {
                let kinds = [idx.lidx.as_ref(), idx.uidx.as_ref()]
                    .into_iter()
                    .flatten()
                    .map(|n| self.eval_expr(n, scope))
                    .collect::<Vec<_>>();
                self.fold_kinds(kinds, scope)
            }
            NodeEnum::ArrayExpr(expr) => {
                let kinds = expr
                    .elements
                    .iter()
                    .map(|n| self.eval_expr(n, scope))
                    .collect::<Vec<_>>();
                self.fold_kinds(kinds, scope)
            }
            NodeEnum::NullIfExpr(expr) => {
                let kinds = expr
                    .args
                    .iter()
                    .map(|n| self.eval_expr(n, scope))
                    .collect::<Vec<_>>();
                self.fold_kinds(kinds, scope)
            }
            NodeEnum::MinMaxExpr(expr) => {
                let kinds = expr
                    .args
                    .iter()
                    .map(|n| self.eval_expr(n, scope))
                    .collect::<Vec<_>>();
                self.fold_kinds(kinds, scope)
            }
            NodeEnum::AArrayExpr(expr) => {
                let kinds = expr
                    .elements
                    .iter()
                    .map(|n| self.eval_expr(n, scope))
                    .collect::<Vec<_>>();
                self.fold_kinds(kinds, scope)
            }
            NodeEnum::AConst(_) | NodeEnum::ParamRef(_) | NodeEnum::SqlvalueFunction(_) => {
                ExprKind::Value
            }
            // Anything not modelled above (`xmlelement`, `JSON_OBJECT`,
            // `IS JSON`, …): fold every column reference and subquery beneath
            // it, so an unmodelled wrapper cannot launder a sensitive value.
            other => self.eval_descendants(other, scope),
        }
    }

    /// `NodeEnum::nodes()` does not descend into every expression type (for
    /// example `XmlExpr`), so walk the node's serde form instead, which covers
    /// every field. A subquery inside an unmodelled node is not analyzed and
    /// is treated as sensitive.
    fn eval_descendants(&mut self, node: &NodeEnum, scope: &[Rel]) -> ExprKind {
        let Ok(json) = serde_json::to_value(node) else {
            return ExprKind::Tainted;
        };
        let mut refs = Vec::new();
        let mut opaque = false;
        collect_column_refs(&json, &mut refs, &mut opaque);
        if opaque {
            return ExprKind::Tainted;
        }
        let kinds = refs
            .iter()
            .map(|col| self.eval_column(col, scope))
            .collect::<Vec<_>>();
        self.fold_kinds(kinds, scope)
    }

    fn eval_case(&mut self, expr: &CaseExpr, scope: &[Rel]) -> ExprKind {
        let mut kinds = Vec::new();
        if let Some(arg) = &expr.arg {
            kinds.push(self.eval_expr(arg, scope));
        }
        for when in &expr.args {
            kinds.push(self.eval_expr(when, scope));
        }
        if let Some(def) = &expr.defresult {
            kinds.push(self.eval_expr(def, scope));
        }
        self.fold_kinds(kinds, scope)
    }

    fn eval_when(&mut self, when: &CaseWhen, scope: &[Rel]) -> ExprKind {
        let mut kinds = Vec::new();
        if let Some(expr) = &when.expr {
            kinds.push(self.eval_expr(expr, scope));
        }
        if let Some(result) = &when.result {
            kinds.push(self.eval_expr(result, scope));
        }
        self.fold_kinds(kinds, scope)
    }

    fn eval_row(&mut self, row: &RowExpr, scope: &[Rel]) -> ExprKind {
        let kinds = row
            .args
            .iter()
            .map(|n| self.eval_expr(n, scope))
            .collect::<Vec<_>>();
        self.fold_kinds(kinds, scope)
    }

    fn eval_sublink(&mut self, sub: &SubLink, scope: &[Rel]) -> ExprKind {
        let mut tainted = false;
        if let Some(test) = &sub.testexpr
            && matches!(self.eval_expr(test, scope), ExprKind::Tainted)
        {
            tainted = true;
        }
        if let Some(query) = &sub.subselect
            && let Some(NodeEnum::SelectStmt(sel)) = query.node.as_ref()
        {
            let cols = self.analyze_select(sel, scope, false);
            if cols.iter().any(|c| c.tainted) {
                tainted = true;
            }
        }
        if tainted {
            ExprKind::Tainted
        } else {
            ExprKind::Value
        }
    }

    fn eval_func(&mut self, func: &FuncCall, scope: &[Rel]) -> ExprKind {
        if func.agg_star {
            return ExprKind::Value;
        }
        let mut kinds = Vec::new();
        for arg in func.args.iter().chain(func.agg_order.iter()) {
            kinds.push(self.eval_expr(arg, scope));
        }
        if let Some(filter) = &func.agg_filter {
            kinds.push(self.eval_expr(filter, scope));
        }
        let name = func_name(func).unwrap_or_default();
        if WHOLE_ROW_FUNCS
            .iter()
            .any(|f| f.eq_ignore_ascii_case(&name))
        {
            for kind in &kinds {
                match kind {
                    ExprKind::WholeRow(idx) => {
                        if let Some(rel) = scope.get(*idx) {
                            self.note_whole_row(rel);
                        }
                    }
                    ExprKind::Star(idxs) => {
                        for idx in idxs {
                            if let Some(rel) = scope.get(*idx) {
                                self.note_whole_row(rel);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        self.fold_kinds(kinds, scope)
    }

    /// Scalar fold. A whole-row or star argument taints the result when the
    /// relation carries sensitive columns (and records a rejection / unresolved
    /// relation). A clean composite does not.
    fn fold_kinds(&mut self, kinds: impl IntoIterator<Item = ExprKind>, scope: &[Rel]) -> ExprKind {
        let mut tainted = false;
        for kind in kinds {
            match kind {
                ExprKind::Value => {}
                ExprKind::Tainted => tainted = true,
                ExprKind::Star(idxs) => {
                    for idx in idxs {
                        if let Some(rel) = scope.get(idx)
                            && self.leaks_whole_row(rel)
                        {
                            self.note_whole_row(rel);
                            tainted = true;
                        }
                    }
                }
                ExprKind::WholeRow(idx) => {
                    if let Some(rel) = scope.get(idx)
                        && self.leaks_whole_row(rel)
                    {
                        self.note_whole_row(rel);
                        tainted = true;
                    }
                }
            }
        }
        if tainted {
            ExprKind::Tainted
        } else {
            ExprKind::Value
        }
    }

    fn eval_column(&self, col: &ColumnRef, scope: &[Rel]) -> ExprKind {
        let mut parts: Vec<Option<&str>> = Vec::new();
        let mut star_at = None;
        for (i, field) in col.fields.iter().enumerate() {
            match field.node.as_ref() {
                Some(NodeEnum::AStar(_)) => star_at = Some(i),
                Some(NodeEnum::String(s)) => parts.push(Some(s.sval.as_str())),
                _ => parts.push(None),
            }
        }
        if star_at == Some(0) && parts.is_empty() {
            return ExprKind::Star((0..scope.len()).collect());
        }
        if star_at == Some(parts.len()) {
            return match parts.as_slice() {
                [Some(rel)] => self.star_of_ident(scope, rel),
                [Some(schema), Some(name)] => self.star_of_qualified(scope, schema, name),
                _ => ExprKind::Star((0..scope.len()).collect()),
            };
        }
        match parts.as_slice() {
            [Some(name)] => self.resolve_unqualified(scope, name),
            [Some(rel), Some(column)] => self.resolve_qualified(scope, rel, column),
            [Some(schema), Some(rel), Some(column)] => {
                self.resolve_schema_column(scope, schema, rel, column)
            }
            _ => ExprKind::Value,
        }
    }

    fn star_of_ident(&self, scope: &[Rel], ident: &str) -> ExprKind {
        match find_rel(scope, ident) {
            Some(idx) => ExprKind::Star(vec![idx]),
            None => ExprKind::Star((0..scope.len()).collect()),
        }
    }

    fn star_of_qualified(&self, scope: &[Rel], schema: &str, name: &str) -> ExprKind {
        match find_qualified(scope, schema, name) {
            Some(idx) => ExprKind::Star(vec![idx]),
            None => ExprKind::Value,
        }
    }

    fn resolve_unqualified(&self, scope: &[Rel], name: &str) -> ExprKind {
        let mut tainted = false;
        let mut found_column = false;
        let mut whole = None;
        for (idx, rel) in scope.iter().enumerate() {
            match self.lookup_column(rel, name) {
                ColumnLookup::Sensitive => {
                    tainted = true;
                    found_column = true;
                }
                ColumnLookup::Plain => found_column = true,
                ColumnLookup::Unknown => {
                    if self.unknown_name_is_sensitive(rel, name) {
                        tainted = true;
                        found_column = true;
                    }
                }
            }
            if rel.alias.eq_ignore_ascii_case(name) && !self.rel_has_known_column(rel, name) {
                whole = Some(idx);
            }
        }
        if tainted {
            ExprKind::Tainted
        } else if found_column {
            ExprKind::Value
        } else if let Some(idx) = whole {
            ExprKind::WholeRow(idx)
        } else {
            ExprKind::Value
        }
    }

    fn resolve_qualified(&self, scope: &[Rel], rel_name: &str, column: &str) -> ExprKind {
        let Some(idx) = find_rel(scope, rel_name) else {
            if self.filter.sensitive_name_heuristics && is_sensitive_column_name(column) {
                return ExprKind::Tainted;
            }
            return ExprKind::Value;
        };
        let rel = &scope[idx];
        if column == "*" {
            return ExprKind::Star(vec![idx]);
        }
        self.sensitivity_of_column(rel, column)
    }

    fn resolve_schema_column(
        &self,
        scope: &[Rel],
        schema: &str,
        rel_name: &str,
        column: &str,
    ) -> ExprKind {
        let Some(idx) = find_qualified(scope, schema, rel_name) else {
            return ExprKind::Value;
        };
        self.sensitivity_of_column(&scope[idx], column)
    }

    fn sensitivity_of_column(&self, rel: &Rel, column: &str) -> ExprKind {
        match self.lookup_column(rel, column) {
            ColumnLookup::Sensitive => ExprKind::Tainted,
            ColumnLookup::Plain => ExprKind::Value,
            ColumnLookup::Unknown => {
                if self.unknown_name_is_sensitive(rel, column) {
                    ExprKind::Tainted
                } else {
                    ExprKind::Value
                }
            }
        }
    }

    fn lookup_column(&self, rel: &Rel, column: &str) -> ColumnLookup {
        match &rel.kind {
            RelKind::Opaque { tainted: true } => ColumnLookup::Sensitive,
            RelKind::Opaque { tainted: false } => ColumnLookup::Unknown,
            RelKind::Derived(cols) => {
                match cols.iter().find(|c| c.name.eq_ignore_ascii_case(column)) {
                    Some(col) if self.derived_is_sensitive(col) => ColumnLookup::Sensitive,
                    Some(_) => ColumnLookup::Plain,
                    None => ColumnLookup::Unknown,
                }
            }
            RelKind::Table => {
                if is_pii_column(&self.filter.pii_columns, &rel.schema, &rel.name, column) {
                    return ColumnLookup::Sensitive;
                }
                match self.known_columns(rel) {
                    Some(cols) => {
                        if cols.iter().any(|c| c.eq_ignore_ascii_case(column)) {
                            if self.filter.sensitive_name_heuristics
                                && is_sensitive_column_name(column)
                            {
                                ColumnLookup::Sensitive
                            } else {
                                ColumnLookup::Plain
                            }
                        } else {
                            ColumnLookup::Unknown
                        }
                    }
                    None => ColumnLookup::Unknown,
                }
            }
        }
    }

    fn unknown_name_is_sensitive(&self, rel: &Rel, column: &str) -> bool {
        if !matches!(rel.kind, RelKind::Table) {
            return false;
        }
        if self.known_columns(rel).is_some() {
            return false;
        }
        is_pii_column(&self.filter.pii_columns, &rel.schema, &rel.name, column)
            || (self.filter.sensitive_name_heuristics && is_sensitive_column_name(column))
    }

    fn rel_has_known_column(&self, rel: &Rel, column: &str) -> bool {
        match &rel.kind {
            RelKind::Derived(cols) => cols.iter().any(|c| c.name.eq_ignore_ascii_case(column)),
            RelKind::Opaque { .. } => false,
            RelKind::Table => self
                .known_columns(rel)
                .is_some_and(|cols| cols.iter().any(|c| c.eq_ignore_ascii_case(column))),
        }
    }

    fn known_columns(&self, rel: &Rel) -> Option<Vec<String>> {
        let cols: Vec<String> = self
            .known
            .iter()
            .filter(|c| eq_ident(&c.schema, &rel.schema) && eq_ident(&c.table, &rel.name))
            .map(|c| c.column.clone())
            .collect();
        if cols.is_empty() { None } else { Some(cols) }
    }

    fn derived_is_sensitive(&self, col: &DerivedCol) -> bool {
        col.tainted
            || (self.filter.sensitive_name_heuristics && is_sensitive_column_name(&col.name))
    }

    fn relation_sensitivity(&self, rel: &Rel) -> Sensitivity {
        match &rel.kind {
            RelKind::Derived(cols) => {
                let names: Vec<String> = cols
                    .iter()
                    .filter(|c| self.derived_is_sensitive(c))
                    .map(|c| c.name.clone())
                    .collect();
                if names.is_empty() {
                    Sensitivity::Clean
                } else {
                    Sensitivity::Names(names)
                }
            }
            RelKind::Opaque { tainted: false } => Sensitivity::Clean,
            RelKind::Opaque { tainted: true } => {
                Sensitivity::Names(vec![OPAQUE_SENSITIVE_COLUMNS.to_owned()])
            }
            RelKind::Table => {
                let pii = configured_pii_names(&self.filter.pii_columns, &rel.schema, &rel.name)
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                match self.known_columns(rel) {
                    Some(cols) => {
                        let mut names: Vec<String> = cols
                            .iter()
                            .filter(|c| {
                                pii.iter().any(|p| p.eq_ignore_ascii_case(c))
                                    || (self.filter.sensitive_name_heuristics
                                        && is_sensitive_column_name(c))
                            })
                            .cloned()
                            .collect();
                        for p in &pii {
                            if !names.iter().any(|n| n.eq_ignore_ascii_case(p)) {
                                names.push(p.clone());
                            }
                        }
                        if names.is_empty() {
                            Sensitivity::Clean
                        } else {
                            Sensitivity::Names(names)
                        }
                    }
                    None if self.filter.sensitive_name_heuristics => {
                        Sensitivity::Unresolved { also: pii }
                    }
                    None if !pii.is_empty() => Sensitivity::Names(pii),
                    None => Sensitivity::Clean,
                }
            }
        }
    }

    fn leaks_whole_row(&self, rel: &Rel) -> bool {
        !matches!(self.relation_sensitivity(rel), Sensitivity::Clean)
    }

    fn note_whole_row(&mut self, rel: &Rel) {
        match self.relation_sensitivity(rel) {
            Sensitivity::Clean => {}
            Sensitivity::Names(names) => self.push_rejection(rel, &names),
            Sensitivity::Unresolved { also } if !also.is_empty() => {
                self.push_rejection(rel, &also);
                self.push_unresolved(rel);
            }
            Sensitivity::Unresolved { .. } => self.push_unresolved(rel),
        }
    }

    fn star_cols(&mut self, rel: &Rel) -> Vec<DerivedCol> {
        if let RelKind::Opaque { tainted } = rel.kind {
            // Output names are unknown, so a sensitive opaque relation cannot
            // be redacted column by column; refuse it instead.
            if tainted {
                self.note_whole_row(rel);
            }
            return Vec::new();
        }
        match self.relation_sensitivity(rel) {
            Sensitivity::Clean => self
                .all_columns(rel)
                .into_iter()
                .map(|name| DerivedCol {
                    name,
                    tainted: false,
                })
                .collect(),
            Sensitivity::Names(sensitive) => self.expand_named(rel, &sensitive),
            Sensitivity::Unresolved { also } => {
                self.push_unresolved(rel);
                also.into_iter()
                    .map(|name| DerivedCol {
                        name,
                        tainted: true,
                    })
                    .collect()
            }
        }
    }

    fn expand_named(&self, rel: &Rel, sensitive: &[String]) -> Vec<DerivedCol> {
        let all = self.all_columns(rel);
        if all.is_empty() {
            return sensitive
                .iter()
                .map(|name| DerivedCol {
                    name: name.clone(),
                    tainted: true,
                })
                .collect();
        }
        all.into_iter()
            .map(|name| {
                let tainted = sensitive.iter().any(|s| s.eq_ignore_ascii_case(&name));
                DerivedCol { name, tainted }
            })
            .collect()
    }

    fn all_columns(&self, rel: &Rel) -> Vec<String> {
        match &rel.kind {
            RelKind::Derived(cols) => cols.iter().map(|c| c.name.clone()).collect(),
            RelKind::Opaque { .. } => Vec::new(),
            RelKind::Table => self.known_columns(rel).unwrap_or_default(),
        }
    }

    fn push_redact(&mut self, name: &str) {
        if !self.redact.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            self.redact.push(name.to_owned());
        }
    }

    fn push_unresolved(&mut self, rel: &Rel) {
        if !matches!(rel.kind, RelKind::Table) {
            return;
        }
        if self
            .unresolved
            .iter()
            .any(|u| eq_ident(&u.schema, &rel.schema) && eq_ident(&u.name, &rel.name))
        {
            return;
        }
        self.unresolved.push(UnresolvedRelation {
            schema: rel.schema.clone(),
            name: rel.name.clone(),
        });
    }

    fn push_rejection(&mut self, rel: &Rel, names: &[String]) {
        let mut names = names.to_vec();
        names.sort_by_key(|name| name.to_ascii_lowercase());
        names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let label = if rel.schema.is_empty() {
            rel.alias.clone()
        } else {
            format!("{}.{}", rel.schema, rel.name)
        };
        let msg = format!(
            "whole-row reference to {label} includes sensitive columns: {}",
            names.join(", ")
        );
        if !self.rejections.contains(&msg) {
            self.rejections.push(msg);
        }
    }
}

enum ColumnLookup {
    Sensitive,
    Plain,
    Unknown,
}

fn is_set_operation(op: i32) -> bool {
    op == pg_query::protobuf::SetOperation::SetopUnion as i32
        || op == pg_query::protobuf::SetOperation::SetopIntersect as i32
        || op == pg_query::protobuf::SetOperation::SetopExcept as i32
}

fn merge_union(mut left: Vec<DerivedCol>, right: Vec<DerivedCol>) -> Vec<DerivedCol> {
    for (idx, col) in right.into_iter().enumerate() {
        if let Some(existing) = left.get_mut(idx) {
            existing.tainted |= col.tainted;
        } else {
            left.push(col);
        }
    }
    left
}

fn apply_cte_aliases(cte: &CommonTableExpr, mut cols: Vec<DerivedCol>) -> Vec<DerivedCol> {
    apply_alias_colnames(&cte.aliascolnames, &mut cols);
    cols
}

fn apply_alias_colnames(aliases: &[Node], cols: &mut [DerivedCol]) {
    for (idx, alias) in aliases.iter().enumerate() {
        if let Some(name) = node_str(alias)
            && let Some(col) = cols.get_mut(idx)
        {
            col.name = name.to_owned();
        }
    }
}

fn find_rel(scope: &[Rel], ident: &str) -> Option<usize> {
    scope
        .iter()
        .position(|rel| rel.alias.eq_ignore_ascii_case(ident))
        .or_else(|| {
            scope
                .iter()
                .position(|rel| rel.name.eq_ignore_ascii_case(ident))
        })
}

fn find_qualified(scope: &[Rel], schema: &str, name: &str) -> Option<usize> {
    scope.iter().position(|rel| {
        eq_ident(&rel.schema, schema)
            && (rel.name.eq_ignore_ascii_case(name) || rel.alias.eq_ignore_ascii_case(name))
    })
}

/// Collect every `ColumnRef` in a serialized node tree. Sets `opaque` when a
/// subquery is found or a `ColumnRef` cannot be rebuilt.
fn collect_column_refs(value: &serde_json::Value, out: &mut Vec<ColumnRef>, opaque: &mut bool) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                match key.as_str() {
                    "ColumnRef" => match column_ref_from_json(child) {
                        Some(col) => out.push(col),
                        None => *opaque = true,
                    },
                    "SubLink" | "SelectStmt" => *opaque = true,
                    _ => collect_column_refs(child, out, opaque),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_column_refs(item, out, opaque);
            }
        }
        _ => {}
    }
}

fn column_ref_from_json(value: &serde_json::Value) -> Option<ColumnRef> {
    let mut fields = Vec::new();
    for field in value.get("fields")?.as_array()? {
        let inner = field.get("node")?;
        let node = if let Some(s) = inner.get("String") {
            NodeEnum::String(pg_query::protobuf::String {
                sval: s.get("sval")?.as_str()?.to_owned(),
            })
        } else if inner.get("AStar").is_some() {
            NodeEnum::AStar(pg_query::protobuf::AStar {})
        } else {
            return None;
        };
        fields.push(Node { node: Some(node) });
    }
    Some(ColumnRef {
        fields,
        location: 0,
    })
}

fn eq_ident(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn node_str(node: &Node) -> Option<&str> {
    match node.node.as_ref()? {
        NodeEnum::String(s) => Some(s.sval.as_str()),
        _ => None,
    }
}

fn func_name(func: &FuncCall) -> Option<String> {
    func.funcname.last().and_then(node_str).map(str::to_owned)
}

/// Result column name PostgreSQL gives an unaliased target. Redaction is by
/// output name, so this must agree with the server or a tainted column slips
/// through; callers still fail closed when a name is not found in the result.
fn expr_name(node: &Node) -> String {
    figure_colname(node)
        .map(|(name, _)| name)
        .unwrap_or_else(|| "?column?".to_owned())
}

/// Port of `FigureColnameInternal` (parse_target.c). The strength is 2 for a
/// real name and 1 for a fallback (`case`, a cast's type name) that an outer
/// cast or CASE may override.
fn figure_colname(node: &Node) -> Option<(String, u8)> {
    let strong = |name: &str| Some((name.to_owned(), 2));
    match node.node.as_ref()? {
        NodeEnum::ColumnRef(col) => col.fields.iter().rev().find_map(node_str).and_then(strong),
        NodeEnum::AIndirection(ind) => match ind.indirection.iter().rev().find_map(node_str) {
            Some(field) => strong(field),
            None => ind.arg.as_deref().and_then(figure_colname),
        },
        NodeEnum::FuncCall(func) => func_name(func).and_then(|name| strong(&name)),
        NodeEnum::AExpr(expr) if expr.kind == AExprKind::AexprNullif as i32 => strong("nullif"),
        NodeEnum::TypeCast(cast) => {
            let inner = cast.arg.as_deref().and_then(figure_colname);
            if inner.as_ref().is_some_and(|(_, strength)| *strength > 1) {
                return inner;
            }
            cast.type_name
                .as_ref()
                .and_then(|t| t.names.iter().rev().find_map(node_str))
                .map(|name| (name.to_owned(), 1))
                .or(inner)
        }
        NodeEnum::CollateClause(col) => col.arg.as_deref().and_then(figure_colname),
        NodeEnum::GroupingFunc(_) => strong("grouping"),
        NodeEnum::MergeSupportFunc(_) => strong("merge_action"),
        NodeEnum::SubLink(sub) => match SubLinkType::try_from(sub.sub_link_type) {
            Ok(SubLinkType::ExistsSublink) => strong("exists"),
            Ok(SubLinkType::ArraySublink) => strong("array"),
            Ok(SubLinkType::ExprSublink) => {
                let name = sub
                    .subselect
                    .as_deref()
                    .and_then(first_target_name)
                    .unwrap_or_else(|| "?column?".to_owned());
                Some((name, 2))
            }
            _ => None,
        },
        NodeEnum::CaseExpr(case) => {
            let def = case.defresult.as_deref().and_then(figure_colname);
            if def.as_ref().is_some_and(|(_, strength)| *strength > 1) {
                def
            } else {
                Some(("case".to_owned(), 1))
            }
        }
        NodeEnum::AArrayExpr(_) => strong("array"),
        NodeEnum::RowExpr(_) => strong("row"),
        NodeEnum::CoalesceExpr(_) => strong("coalesce"),
        NodeEnum::MinMaxExpr(expr) => match MinMaxOp::try_from(expr.op) {
            Ok(MinMaxOp::IsGreatest) => strong("greatest"),
            Ok(MinMaxOp::IsLeast) => strong("least"),
            _ => None,
        },
        NodeEnum::XmlExpr(expr) => match XmlExprOp::try_from(expr.op) {
            Ok(XmlExprOp::IsXmlconcat) => strong("xmlconcat"),
            Ok(XmlExprOp::IsXmlelement) => strong("xmlelement"),
            Ok(XmlExprOp::IsXmlforest) => strong("xmlforest"),
            Ok(XmlExprOp::IsXmlparse) => strong("xmlparse"),
            Ok(XmlExprOp::IsXmlpi) => strong("xmlpi"),
            Ok(XmlExprOp::IsXmlroot) => strong("xmlroot"),
            Ok(XmlExprOp::IsXmlserialize) => strong("xmlserialize"),
            _ => None,
        },
        NodeEnum::XmlSerialize(_) => strong("xmlserialize"),
        NodeEnum::JsonObjectConstructor(_) => strong("json_object"),
        NodeEnum::JsonArrayConstructor(_) | NodeEnum::JsonArrayQueryConstructor(_) => {
            strong("json_array")
        }
        NodeEnum::JsonObjectAgg(_) => strong("json_objectagg"),
        NodeEnum::JsonArrayAgg(_) => strong("json_arrayagg"),
        NodeEnum::JsonParseExpr(_) => strong("json"),
        NodeEnum::JsonScalarExpr(_) => strong("json_scalar"),
        NodeEnum::JsonSerializeExpr(_) => strong("json_serialize"),
        NodeEnum::JsonFuncExpr(expr) => match JsonExprOp::try_from(expr.op) {
            Ok(JsonExprOp::JsonExistsOp) => strong("json_exists"),
            Ok(JsonExprOp::JsonQueryOp) => strong("json_query"),
            Ok(JsonExprOp::JsonValueOp) => strong("json_value"),
            _ => None,
        },
        _ => None,
    }
}

/// Name of a scalar subquery's single output column (its leftmost arm for a
/// set operation), which PostgreSQL reuses as the outer column name.
fn first_target_name(node: &Node) -> Option<String> {
    let NodeEnum::SelectStmt(sel) = node.node.as_ref()? else {
        return None;
    };
    if is_set_operation(sel.op) {
        return sel.larg.as_ref().and_then(|larg| {
            first_target_name(&Node {
                node: Some(NodeEnum::SelectStmt(larg.clone())),
            })
        });
    }
    let NodeEnum::ResTarget(rt) = sel.target_list.first()?.node.as_ref()? else {
        return None;
    };
    if !rt.name.is_empty() {
        return Some(rt.name.clone());
    }
    Some(
        rt.val
            .as_deref()
            .map(expr_name)
            .unwrap_or_else(|| "?column?".to_owned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pii_filter() -> PolicyFilter {
        PolicyFilter {
            pii_columns: vec!["public.users.email".into(), "public.users.phone".into()],
            ..Default::default()
        }
    }

    fn known_users() -> Vec<CatalogColumn> {
        ["email", "phone", "password_hash", "id"]
            .into_iter()
            .map(|column| CatalogColumn {
                schema: "public".into(),
                table: "users".into(),
                column: column.into(),
            })
            .collect()
    }

    fn redact(sql: &str) -> Vec<String> {
        analyze_output_lineage(sql, &pii_filter(), &known_users())
            .unwrap()
            .redact_names
    }

    #[test]
    fn alias_of_pii_column_is_redacted() {
        let names = redact("SELECT email AS contact FROM users");
        assert!(names.iter().any(|n| n == "contact"), "{names:?}");
    }

    #[test]
    fn function_wrapping_pii_is_redacted() {
        let names = redact("SELECT lower(email) FROM users");
        assert!(names.iter().any(|n| n == "lower"), "{names:?}");
    }

    #[test]
    fn concat_and_case_carry_taint() {
        assert!(
            redact("SELECT concat(email, phone) FROM users")
                .iter()
                .any(|n| n == "concat")
        );
        assert!(
            redact("SELECT CASE WHEN id = 1 THEN email ELSE phone END FROM users")
                .iter()
                .any(|n| n == "phone"),
            "PostgreSQL names a CASE after its ELSE column"
        );
    }

    #[test]
    fn subquery_and_cte_and_lateral_carry_taint() {
        assert!(
            redact("SELECT (SELECT email FROM users) AS contact")
                .iter()
                .any(|n| n == "contact"),
        );
        assert!(
            redact("WITH u AS (SELECT email FROM users) SELECT email AS contact FROM u")
                .iter()
                .any(|n| n == "contact"),
        );
        assert!(
            redact("SELECT l.contact FROM users u, LATERAL (SELECT u.email AS contact) l")
                .iter()
                .any(|n| n == "contact"),
        );
    }

    #[test]
    fn row_to_json_of_a_sensitive_relation_is_rejected() {
        let lineage = analyze_output_lineage(
            "SELECT row_to_json(u) FROM users u",
            &pii_filter(),
            &known_users(),
        )
        .unwrap();
        let msg = lineage.rejections.join(" ");
        assert!(
            msg.contains("password_hash") || msg.contains("email"),
            "{msg}"
        );
    }

    #[test]
    fn star_redacts_known_sensitive_columns() {
        let names = redact("SELECT * FROM users");
        for expected in ["email", "phone", "password_hash"] {
            assert!(names.iter().any(|n| n == expected), "{names:?}");
        }
        assert!(!names.iter().any(|n| n == "id"), "{names:?}");
    }

    #[test]
    fn heuristics_redact_password_hash_without_configured_pii() {
        let filter = PolicyFilter::default();
        let names =
            analyze_output_lineage("SELECT password_hash FROM users", &filter, &known_users())
                .unwrap()
                .redact_names;
        assert!(names.iter().any(|n| n == "password_hash"), "{names:?}");
    }

    #[test]
    fn heuristics_can_be_disabled() {
        let filter = PolicyFilter {
            sensitive_name_heuristics: false,
            ..Default::default()
        };
        let lineage =
            analyze_output_lineage("SELECT password_hash FROM users", &filter, &known_users())
                .unwrap();
        assert!(
            lineage.redact_names.is_empty(),
            "{:?}",
            lineage.redact_names
        );
        assert!(lineage.rejections.is_empty());
    }

    #[test]
    fn whole_row_without_catalog_is_unresolved_when_heuristics_are_on() {
        let lineage = analyze_output_lineage(
            "SELECT row_to_json(u) FROM users u",
            &PolicyFilter::default(),
            &[],
        )
        .unwrap();
        assert!(lineage.rejections.is_empty(), "{:?}", lineage.rejections);
        assert!(
            lineage
                .unresolved_whole_rows
                .iter()
                .any(|r| r.name == "users"),
            "{:?}",
            lineage.unresolved_whole_rows
        );
    }

    fn lineage(sql: &str) -> OutputLineage {
        analyze_output_lineage(sql, &pii_filter(), &known_users()).unwrap()
    }

    #[test]
    fn function_in_from_cannot_launder_a_whole_row() {
        let l = lineage(
            "SELECT kv.key, kv.value FROM users u \
             CROSS JOIN LATERAL jsonb_each_text(to_jsonb(u)) kv",
        );
        assert!(!l.rejections.is_empty(), "{l:?}");
    }

    #[test]
    fn function_in_from_fed_by_pii_taints_its_columns() {
        let l = lineage(
            "SELECT kv.value FROM users u, jsonb_each_text(jsonb_build_object('e', u.email)) kv",
        );
        assert!(l.redact_names.iter().any(|n| n == "value"), "{l:?}");
        let star = lineage(
            "SELECT kv.* FROM users u, jsonb_each_text(jsonb_build_object('e', u.email)) kv",
        );
        assert!(!star.rejections.is_empty(), "{star:?}");
    }

    #[test]
    fn clean_function_in_from_stays_clean() {
        let l = lineage("SELECT g FROM generate_series(1, 3) g");
        assert!(l.is_clean(), "{l:?}");
    }

    #[test]
    fn tablesample_relation_is_still_tracked() {
        assert_eq!(
            redact("SELECT email AS e FROM users TABLESAMPLE SYSTEM (50)"),
            vec!["e"]
        );
    }

    #[test]
    fn unaliased_outputs_use_postgres_column_names() {
        let cases = [
            (
                "SELECT (SELECT email AS contact FROM users LIMIT 1)",
                "contact",
            ),
            ("SELECT (SELECT email FROM users LIMIT 1)", "email"),
            (
                "SELECT CASE WHEN id > 0 THEN '' ELSE email END FROM users",
                "email",
            ),
            ("SELECT CASE WHEN id > 0 THEN email END FROM users", "case"),
            ("SELECT (email || '')::text FROM users", "text"),
            ("SELECT email::varchar FROM users", "email"),
            ("SELECT ARRAY[email] FROM users", "array"),
            ("SELECT xmlelement(name x, email) FROM users", "xmlelement"),
            ("SELECT greatest(email, 'z') FROM users", "greatest"),
            ("SELECT nullif(email, '') FROM users", "nullif"),
            ("SELECT email || '' FROM users", "?column?"),
        ];
        for (sql, expected) in cases {
            let names = redact(sql);
            assert!(names.iter().any(|n| n == expected), "{sql}: {names:?}");
        }
    }

    #[test]
    fn unqualified_relations_resolve_through_search_path() {
        let filter = PolicyFilter {
            pii_columns: vec!["customers.users.email".into()],
            ..Default::default()
        };
        let sql = "SELECT email AS c FROM users";
        let first = analyze_output_lineage(sql, &filter, &[]).unwrap();
        assert_eq!(first.unqualified_relations, vec!["users"]);
        assert!(first.redact_names.is_empty(), "{first:?}");

        let schemas = HashMap::from([("users".to_owned(), "customers".to_owned())]);
        let resolved = analyze_output_lineage_resolved(sql, &filter, &[], &schemas).unwrap();
        assert_eq!(resolved.redact_names, vec!["c"]);
        assert!(resolved.unqualified_relations.is_empty());
    }

    #[test]
    fn catalog_view_star_is_not_assumed_public() {
        let schemas = HashMap::from([("pg_tables".to_owned(), "pg_catalog".to_owned())]);
        let l = analyze_output_lineage_resolved(
            "SELECT * FROM pg_tables",
            &PolicyFilter::default(),
            &[],
            &schemas,
        )
        .unwrap();
        assert!(
            l.unresolved_whole_rows
                .iter()
                .all(|r| r.schema == "pg_catalog"),
            "{l:?}"
        );
    }

    #[test]
    fn recursive_cte_carries_taint_through_the_self_reference() {
        let names = redact(
            "WITH RECURSIVE r(n, e) AS (\
               SELECT 1, ''::text \
               UNION ALL SELECT n + 1, (SELECT email FROM users LIMIT 1) FROM r WHERE n < 2\
             ) SELECT e AS out FROM r",
        );
        assert!(names.iter().any(|n| n == "out"), "{names:?}");
    }

    #[test]
    fn explain_does_not_ask_for_column_redaction() {
        let l = lineage("EXPLAIN SELECT email AS contact FROM users");
        assert!(l.redact_names.is_empty(), "{l:?}");
    }
}
