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
    CaseExpr, CaseWhen, ColumnRef, CommonTableExpr, FuncCall, Node, RangeSubselect, RangeVar,
    ResTarget, RowExpr, SelectStmt, SubLink,
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
/// heuristics are on.
pub fn analyze_output_lineage(
    sql: &str,
    filter: &PolicyFilter,
    known_columns: &[CatalogColumn],
) -> Result<OutputLineage, PolicyError> {
    let parsed = parse(sql.trim()).map_err(|e| PolicyError::SqlParse(e.to_string()))?;
    let mut analysis = Analysis {
        filter,
        known: known_columns,
        ctes: HashMap::new(),
        redact: Vec::new(),
        rejections: Vec::new(),
        unresolved: Vec::new(),
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
                if let Some(NodeEnum::SelectStmt(sel)) =
                    explain.query.as_ref().and_then(|n| n.node.as_ref())
                {
                    analysis.analyze_select(sel, &[], true);
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
    ctes: HashMap<String, Vec<DerivedCol>>,
    redact: Vec<String>,
    rejections: Vec<String>,
    unresolved: Vec<UnresolvedRelation>,
}

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
                self.define_cte(cte);
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

    fn define_cte(&mut self, node: &Node) {
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
        let _ = self.analyze_select(sel, &[], false);
        let cols = self.analyze_select(sel, &[], false);
        self.ctes.insert(key, apply_cte_aliases(cte, cols));
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
            _ => {}
        }
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

    fn rel_from_range(&self, rv: &RangeVar) -> Rel {
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
        let schema = if rv.schemaname.is_empty() {
            "public".to_owned()
        } else {
            rv.schemaname.clone()
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
            _ => ExprKind::Value,
        }
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
            RelKind::Table => self.known_columns(rel).unwrap_or_default(),
        }
    }

    fn push_redact(&mut self, name: &str) {
        if !self.redact.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            self.redact.push(name.to_owned());
        }
    }

    fn push_unresolved(&mut self, rel: &Rel) {
        if matches!(rel.kind, RelKind::Derived(_)) {
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

fn expr_name(node: &Node) -> String {
    match node.node.as_ref() {
        Some(NodeEnum::ColumnRef(col)) => col
            .fields
            .iter()
            .rev()
            .find_map(node_str)
            .unwrap_or("?column?")
            .to_owned(),
        Some(NodeEnum::FuncCall(func)) => func_name(func).unwrap_or_else(|| "?column?".to_owned()),
        Some(NodeEnum::CaseExpr(_)) | Some(NodeEnum::CaseWhen(_)) => "case".to_owned(),
        Some(NodeEnum::RowExpr(_)) => "row".to_owned(),
        Some(NodeEnum::CoalesceExpr(_)) => "coalesce".to_owned(),
        Some(NodeEnum::TypeCast(cast)) => cast
            .arg
            .as_ref()
            .map(|n| expr_name(n))
            .unwrap_or_else(|| "?column?".to_owned()),
        Some(NodeEnum::CollateClause(col)) => col
            .arg
            .as_ref()
            .map(|n| expr_name(n))
            .unwrap_or_else(|| "?column?".to_owned()),
        _ => "?column?".to_owned(),
    }
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
                .any(|n| n == "case")
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
}
