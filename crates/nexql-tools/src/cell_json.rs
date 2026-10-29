// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Typed Postgres cell → JSON conversion shared by read and write tools.

use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use nexql_policy::{
    CatalogColumn, ObjectRef, PII_REDACTED, PolicyFilter, UnresolvedRelation,
    analyze_output_lineage_resolved, column_matches_pii_policy, column_should_redact,
    is_sensitive_column_name,
};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio_postgres::types::{FromSql, Kind, Type};
use uuid::Uuid;

/// Convert one query row to a JSON object keyed by column name.
pub fn row_to_json(row: &tokio_postgres::Row) -> Value {
    let mut map = serde_json::Map::new();
    for (i, col) in row.columns().iter().enumerate() {
        map.insert(col.name().to_string(), cell_to_json(row, i));
    }
    Value::Object(map)
}

/// Convert query rows to a JSON array of objects.
pub fn rows_to_json_vec(rows: &[tokio_postgres::Row]) -> Vec<Value> {
    rows.iter().map(row_to_json).collect()
}

/// Convert query rows to a JSON array value (read-tool shape).
pub fn rows_to_json_array(rows: &[tokio_postgres::Row]) -> Value {
    Value::Array(rows_to_json_vec(rows))
}

/// Like [`rows_to_json_array`] but strips the pagination helper column and returns total count.
pub fn rows_to_json_array_with_total(
    rows: &[tokio_postgres::Row],
    total_col: &str,
) -> (Option<i64>, Value) {
    let total = rows.first().and_then(|row| {
        row.columns().iter().enumerate().find_map(|(i, col)| {
            if col.name() == total_col {
                row.try_get::<_, Option<i64>>(i).ok().flatten()
            } else {
                None
            }
        })
    });
    let values: Vec<Value> = rows
        .iter()
        .map(|row| row_to_json_excluding(row, total_col))
        .collect();
    (total, Value::Array(values))
}

fn row_to_json_excluding(row: &tokio_postgres::Row, skip: &str) -> Value {
    let mut map = serde_json::Map::new();
    for (i, col) in row.columns().iter().enumerate() {
        if col.name() == skip {
            continue;
        }
        map.insert(col.name().to_string(), cell_to_json(row, i));
    }
    Value::Object(map)
}

/// Build the columnar envelope `{"columns": [...], "rows": [[...], ...],
/// "allNullColumns": [...]}` from a homogeneous array of row objects (all
/// same key set — caller guarantees this). All-null columns are dropped from
/// the row arrays and listed once instead of repeating null per row.
fn columnar_from_object_array(rows: Vec<Value>) -> Value {
    let Some(first) = rows.first().and_then(|r| r.as_object()) else {
        return json!({ "columns": [], "rows": rows });
    };
    let column_names: Vec<String> = first.keys().cloned().collect();

    let mut all_null = vec![true; column_names.len()];
    let mut grid: Vec<Vec<Value>> = Vec::with_capacity(rows.len());
    for row in &rows {
        let as_obj = row.as_object();
        let mut r = Vec::with_capacity(column_names.len());
        for (i, name) in column_names.iter().enumerate() {
            let v = as_obj
                .and_then(|m| m.get(name))
                .cloned()
                .unwrap_or(Value::Null);
            if !v.is_null() {
                all_null[i] = false;
            }
            r.push(v);
        }
        grid.push(r);
    }

    let kept: Vec<usize> = (0..column_names.len()).filter(|&i| !all_null[i]).collect();
    let dropped_names: Vec<&String> = (0..column_names.len())
        .filter(|&i| all_null[i])
        .map(|i| &column_names[i])
        .collect();
    let kept_names: Vec<&String> = kept.iter().map(|&i| &column_names[i]).collect();
    if kept.len() != column_names.len() {
        grid = grid
            .into_iter()
            .map(|row| kept.iter().map(|&i| row[i].clone()).collect())
            .collect();
    }

    let mut out = json!({ "columns": kept_names, "rows": grid });
    if !dropped_names.is_empty() {
        out["allNullColumns"] = json!(dropped_names);
    }
    out
}

/// Reshape a `{"rows": [{...}, ...], ...}` read-tool payload (already
/// PII-redacted — this is a pure reshape, it knows nothing about policy) from
/// per-row objects into columnar form: `{"columns": [...], "rows": [[...],
/// ...], ...}`. Typically 3–5x fewer tokens at higher row/column counts,
/// since column names aren't repeated per row. Sibling keys on the payload
/// object (e.g. `truncated`, `maxRows`) are left untouched. A no-op on
/// anything that isn't `{"rows": [{...}]}` shaped.
pub fn columnarize_read_payload(mut payload: Value) -> Value {
    let Some(obj) = payload.as_object_mut() else {
        return payload;
    };
    let Some(Value::Array(rows)) = obj.remove("rows") else {
        return payload;
    };
    if rows.first().and_then(|r| r.as_object()).is_none() {
        // Empty result set or already non-object rows — put back untouched.
        obj.insert("columns".into(), json!([]));
        obj.insert("rows".into(), Value::Array(rows));
        return payload;
    }
    let columnar = columnar_from_object_array(rows);
    if let Some(cobj) = columnar.as_object() {
        for (k, v) in cobj {
            obj.insert(k.clone(), v.clone());
        }
    }
    payload
}

/// Every object in `items` has the exact same key set (order-insensitive) —
/// the "these are genuinely rows of one result set" check. An empty slice or
/// any non-object element fails this.
fn is_uniform_object_array(items: &[Value]) -> bool {
    let Some(first) = items.first().and_then(|v| v.as_object()) else {
        return false;
    };
    let first_keys: std::collections::BTreeSet<&str> = first.keys().map(String::as_str).collect();
    items.iter().all(|item| {
        item.as_object().is_some_and(|obj| {
            let keys: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
            keys == first_keys
        })
    })
}

/// Recursively reshape every non-empty, uniform (identical key set) array of
/// objects found anywhere in `value` into the columnar envelope — the
/// global-default half of Issue 5. Arrays of non-objects, empty arrays, and
/// arrays of objects with differing key sets (heterogeneous data — nothing
/// downstream should assume they're "rows") are left exactly as they are.
///
/// This is intentionally more aggressive than [`columnarize_read_payload`]
/// (which only ever looks at the top-level `"rows"` key): call sites that
/// build genuinely non-tabular arrays of uniform-shaped objects (a hand
/// -curated summary list, a route/path) should exclude themselves at the
/// caller rather than rely on this function to know the difference — see
/// `ToolRouter::call`'s exclusion list for `orient` / `get_join_path`.
pub fn columnarize_row_arrays(value: Value) -> Value {
    match value {
        Value::Array(items) => {
            if is_uniform_object_array(&items) {
                columnar_from_object_array(items)
            } else {
                Value::Array(items.into_iter().map(columnarize_row_arrays).collect())
            }
        }
        Value::Object(mut map) => {
            // A `"rows"` key holding a uniform array-of-objects is the
            // common bare-{"rows": [...]}-payload case (list_extensions,
            // list_running_queries, ...) — flatten its columnar envelope
            // into this object directly (top-level "columns"/"rows"),
            // matching columnarize_read_payload's shape for run_select,
            // instead of nesting under "rows.rows". Must happen *before*
            // the generic per-value recursion below, which would otherwise
            // already have turned it into a plain object by the time this
            // check runs.
            if let Some(Value::Array(rows)) = map.get("rows")
                && is_uniform_object_array(rows)
            {
                let Some(Value::Array(rows)) = map.remove("rows") else {
                    unreachable!("just matched Value::Array above")
                };
                if let Some(cobj) = columnar_from_object_array(rows).as_object() {
                    for (k, v) in cobj {
                        map.insert(k.clone(), v.clone());
                    }
                }
            }
            Value::Object(
                map.into_iter()
                    .map(|(k, v)| (k, columnarize_row_arrays(v)))
                    .collect(),
            )
        }
        other => other,
    }
}

/// Redact configured PII columns in row objects. Returns redacted JSON and column names touched.
pub fn redact_pii_in_rows(
    rows: Vec<Value>,
    pii_columns: &[String],
    tables: &[ObjectRef],
) -> (Vec<Value>, Vec<String>) {
    if pii_columns.is_empty() || tables.is_empty() {
        return (rows, Vec::new());
    }
    let mut redacted_cols = Vec::new();
    let out = rows
        .into_iter()
        .map(|row| {
            let mut obj = match row {
                Value::Object(map) => map,
                other => return other,
            };
            for (col, val) in obj.iter_mut() {
                if column_matches_pii_policy(pii_columns, tables, col) {
                    *val = Value::String(PII_REDACTED.into());
                    if !redacted_cols.iter().any(|c| c == col) {
                        redacted_cols.push(col.clone());
                    }
                }
            }
            Value::Object(obj)
        })
        .collect();
    (out, redacted_cols)
}

/// Redact PII inside a structured read payload (`{ "rows": [...] }` or a bare array).
pub fn redact_pii_in_payload(
    mut payload: Value,
    pii_columns: &[String],
    tables: &[ObjectRef],
) -> (Value, Vec<String>) {
    if let Some(rows) = payload.get_mut("rows").and_then(|v| v.as_array_mut()) {
        let taken = std::mem::take(rows);
        let (redacted, cols) = redact_pii_in_rows(taken, pii_columns, tables);
        *rows = redacted;
        return (payload, cols);
    }
    if let Value::Array(rows) = &mut payload {
        let taken = std::mem::take(rows);
        let (redacted, cols) = redact_pii_in_rows(taken, pii_columns, tables);
        *rows = redacted;
        return (payload, cols);
    }
    (payload, Vec::new())
}

/// Redact result columns by output name (lineage aliases, not just physical names).
pub fn redact_named_columns_in_payload(
    mut payload: Value,
    names: &[String],
) -> (Value, Vec<String>) {
    if names.is_empty() {
        return (payload, Vec::new());
    }
    let hit = |key: &str| names.iter().any(|name| name.eq_ignore_ascii_case(key));
    if let Some(rows) = payload.get_mut("rows").and_then(|v| v.as_array_mut()) {
        let taken = std::mem::take(rows);
        let (redacted, cols) = redact_named_rows(taken, hit);
        *rows = redacted;
        return (payload, cols);
    }
    if let Value::Array(rows) = &mut payload {
        let taken = std::mem::take(rows);
        let (redacted, cols) = redact_named_rows(taken, hit);
        *rows = redacted;
        return (payload, cols);
    }
    (payload, Vec::new())
}

fn redact_named_rows(rows: Vec<Value>, hit: impl Fn(&str) -> bool) -> (Vec<Value>, Vec<String>) {
    let mut redacted_cols: Vec<String> = Vec::new();
    let out = rows
        .into_iter()
        .map(|row| {
            let mut obj = match row {
                Value::Object(map) => map,
                other => return other,
            };
            for (col, val) in obj.iter_mut() {
                if hit(col) {
                    *val = Value::String(PII_REDACTED.into());
                    if !redacted_cols.iter().any(|c| c.eq_ignore_ascii_case(col)) {
                        redacted_cols.push(col.clone());
                    }
                }
            }
            Value::Object(obj)
        })
        .collect();
    (out, redacted_cols)
}

const RELATION_COLUMNS_SQL: &str = "\
SELECT n.nspname::text, c.relname::text, a.attname::text
FROM unnest($1::text[], $2::text[]) AS x(schema_name, table_name)
JOIN pg_namespace n ON n.nspname = x.schema_name
JOIN pg_class c ON c.relnamespace = n.oid AND c.relname = x.table_name
JOIN pg_attribute a ON a.attrelid = c.oid
WHERE a.attnum > 0 AND NOT a.attisdropped";

const OUTPUT_COLUMN_META_SQL: &str = "\
SELECT x.ord::int4, n.nspname::text, c.relname::text, a.attname::text
FROM unnest($1::text[], $2::text[]) WITH ORDINALITY AS x(oid_text, att_text, ord)
JOIN pg_attribute a ON a.attrelid = x.oid_text::oid AND a.attnum = x.att_text::int2
JOIN pg_class c ON c.oid = a.attrelid
JOIN pg_namespace n ON n.oid = c.relnamespace
ORDER BY x.ord";

/// Schema the session `search_path` resolves each unqualified name to
/// (`pg_tables` → `pg_catalog`). Names that do not resolve are left out.
const SEARCH_PATH_SCHEMAS_SQL: &str = "\
SELECT x.name, n.nspname::text
FROM unnest($1::text[]) AS x(name)
JOIN pg_class c ON c.oid = to_regclass(quote_ident(x.name))
JOIN pg_namespace n ON n.oid = c.relnamespace";

/// Column-lineage redaction names for `sql`. Resolves unqualified relations
/// through the session `search_path`, and loads `pg_attribute` when a
/// whole-row or star reference cannot be classified from config alone.
/// Whole-row constructs over sensitive relations return an error.
pub async fn sensitive_output_names(
    client: &tokio_postgres::Client,
    sql: &str,
    filter: &PolicyFilter,
) -> Result<Vec<String>, crate::error::ToolError> {
    let analyze = |known: &[CatalogColumn], schemas: &HashMap<String, String>| {
        analyze_output_lineage_resolved(sql, filter, known, schemas)
            .map_err(|err| crate::error::ToolError::Execution(err.to_string()))
    };
    let mut known = Vec::new();
    let mut schemas = HashMap::new();
    let mut lineage = analyze(&known, &schemas)?;
    if !lineage.unqualified_relations.is_empty() {
        schemas = resolve_search_path_schemas(client, &lineage.unqualified_relations).await;
        if !schemas.is_empty() {
            lineage = analyze(&known, &schemas)?;
        }
    }
    if !lineage.unresolved_whole_rows.is_empty() {
        known = load_relation_columns(client, &lineage.unresolved_whole_rows).await;
        lineage = analyze(&known, &schemas)?;
    }
    if !lineage.rejections.is_empty() {
        return Err(crate::error::ToolError::Execution(
            lineage.rejections.join("; "),
        ));
    }
    if !lineage.unresolved_whole_rows.is_empty() {
        let rels = lineage
            .unresolved_whole_rows
            .iter()
            .map(|rel| format!("{}.{}", rel.schema, rel.name))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(crate::error::ToolError::Execution(format!(
            "refusing whole-row output over {rels}: column list is unknown, so sensitive values could leak"
        )));
    }
    Ok(lineage.redact_names)
}

async fn resolve_search_path_schemas(
    client: &tokio_postgres::Client,
    names: &[String],
) -> HashMap<String, String> {
    match client.query(SEARCH_PATH_SCHEMAS_SQL, &[&names]).await {
        Ok(rows) => rows
            .into_iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
            .collect(),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "search_path lookup for PII lineage failed; unqualified relations assumed public"
            );
            HashMap::new()
        }
    }
}

/// Fail closed when a sensitive output name from lineage is not one of the
/// result's columns: redaction is by name, so a mismatch would mean the
/// sensitive column went out unredacted under some other name.
pub fn ensure_redaction_names_present(
    lineage_names: &[String],
    result_columns: &[&str],
) -> Result<(), crate::error::ToolError> {
    if result_columns.is_empty() {
        return Ok(());
    }
    let missing: Vec<&str> = lineage_names
        .iter()
        .filter(|name| {
            !result_columns
                .iter()
                .any(|col| col.eq_ignore_ascii_case(name))
        })
        .map(String::as_str)
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(crate::error::ToolError::Execution(format!(
        "could not match sensitive output column(s) {} to the result columns; give each \
         expression an explicit alias (AS name) and retry",
        missing.join(", ")
    )))
}

async fn load_relation_columns(
    client: &tokio_postgres::Client,
    relations: &[UnresolvedRelation],
) -> Vec<CatalogColumn> {
    if relations.is_empty() {
        return Vec::new();
    }
    let schemas: Vec<String> = relations.iter().map(|rel| rel.schema.clone()).collect();
    let tables: Vec<String> = relations.iter().map(|rel| rel.name.clone()).collect();
    let rows = match client
        .query(RELATION_COLUMNS_SQL, &[&schemas, &tables])
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "pg_attribute lookup for PII lineage failed; whole-row output stays unresolved"
            );
            return Vec::new();
        }
    };
    rows.into_iter()
        .map(|row| CatalogColumn {
            schema: row.get(0),
            table: row.get(1),
            column: row.get(2),
        })
        .collect()
}

/// Extra redaction from `RowDescription` table OID + attnum, when the driver
/// populated them. Output names (aliases) are what get redacted.
pub async fn metadata_sensitive_output_names(
    client: &tokio_postgres::Client,
    rows: &[tokio_postgres::Row],
    filter: &PolicyFilter,
) -> Vec<String> {
    let Some(row) = rows.first() else {
        return Vec::new();
    };
    let mut oids = Vec::new();
    let mut attnums = Vec::new();
    let mut output_names = Vec::new();
    for col in row.columns() {
        let Some(oid) = col.table_oid().filter(|oid| *oid != 0) else {
            continue;
        };
        let Some(attnum) = col.column_id().filter(|attnum| *attnum > 0) else {
            continue;
        };
        oids.push(oid.to_string());
        attnums.push(attnum.to_string());
        output_names.push(col.name().to_owned());
    }
    if oids.is_empty() {
        return Vec::new();
    }
    let found = match client
        .query(OUTPUT_COLUMN_META_SQL, &[&oids, &attnums])
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "result-column metadata lookup failed; AST lineage redaction still applies"
            );
            return Vec::new();
        }
    };
    let mut redact: Vec<String> = Vec::new();
    for meta in found {
        let ord: i32 = meta.get(0);
        let schema: String = meta.get(1);
        let table: String = meta.get(2);
        let column: String = meta.get(3);
        let idx = usize::try_from(ord).unwrap_or(0).saturating_sub(1);
        if column_should_redact(filter, &[ObjectRef::new(schema, table)], &column)
            && let Some(output) = output_names.get(idx)
            && !redact.iter().any(|name| name.eq_ignore_ascii_case(output))
        {
            redact.push(output.clone());
        }
    }
    redact
}

/// Output keys that match the sensitive-name heuristics, regardless of lineage.
pub fn heuristic_output_names(payload: &Value, filter: &PolicyFilter) -> Vec<String> {
    if !filter.sensitive_name_heuristics {
        return Vec::new();
    }
    let mut names = Vec::new();
    let rows = payload
        .get("rows")
        .and_then(|v| v.as_array())
        .or_else(|| payload.as_array());
    let Some(rows) = rows else {
        return names;
    };
    for row in rows {
        let Some(obj) = row.as_object() else {
            continue;
        };
        for key in obj.keys() {
            if is_sensitive_column_name(key)
                && !names.iter().any(|name| name.eq_ignore_ascii_case(key))
            {
                names.push(key.clone());
            }
        }
    }
    names
}

/// Detect SQL NULL for any column type without committing to a concrete `FromSql` type.
enum SqlNullness {
    Null,
    Value,
}

impl<'a> FromSql<'a> for SqlNullness {
    fn from_sql(_: &Type, _: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(SqlNullness::Value)
    }

    fn from_sql_null(_: &Type) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(SqlNullness::Null)
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

fn try_cell<T, F>(row: &tokio_postgres::Row, idx: usize, map: F) -> Option<Value>
where
    T: for<'a> FromSql<'a>,
    F: FnOnce(T) -> Value,
{
    match row.try_get::<_, Option<T>>(idx) {
        Ok(Some(v)) => Some(map(v)),
        Ok(None) => Some(Value::Null),
        Err(_) => None,
    }
}

pub fn cell_to_json(row: &tokio_postgres::Row, idx: usize) -> Value {
    let col_type = row.columns()[idx].type_();
    if matches!(row.try_get::<_, SqlNullness>(idx), Ok(SqlNullness::Null)) {
        return Value::Null;
    }

    if let Kind::Array(elem) = col_type.kind() {
        return array_cell_to_json(row, idx, elem);
    }

    if let Some(v) = match *col_type {
        Type::BOOL => try_cell::<bool, _>(row, idx, |b| json!(b)),
        Type::INT2 => try_cell::<i16, _>(row, idx, |n| json!(n)),
        Type::INT4 | Type::OID => try_cell::<i32, _>(row, idx, |n| json!(n)),
        Type::INT8 => try_cell::<i64, _>(row, idx, |n| json!(n)),
        Type::FLOAT4 => try_cell::<f32, _>(row, idx, |n| json!(n)),
        Type::FLOAT8 => try_cell::<f64, _>(row, idx, |n| json!(n)),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => {
            try_cell::<String, _>(row, idx, Value::String)
        }
        Type::TIMESTAMP => try_cell::<NaiveDateTime, _>(row, idx, |t| {
            json!(t.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
        }),
        Type::TIMESTAMPTZ => {
            try_cell::<DateTime<FixedOffset>, _>(row, idx, |t| json!(t.to_rfc3339()))
        }
        Type::DATE => {
            try_cell::<NaiveDate, _>(row, idx, |d| json!(d.format("%Y-%m-%d").to_string()))
        }
        Type::TIME => {
            try_cell::<NaiveTime, _>(row, idx, |t| json!(t.format("%H:%M:%S%.f").to_string()))
        }
        Type::UUID => try_cell::<Uuid, _>(row, idx, |u| json!(u.to_string())),
        Type::JSON | Type::JSONB => try_cell::<Value, _>(row, idx, |j| j),
        Type::NUMERIC => try_cell::<Decimal, _>(row, idx, |d| json!(d.to_string())),
        Type::MONEY => try_cell::<i64, _>(row, idx, |v| json!(money_to_string(v))),
        Type::BYTEA => try_cell::<Vec<u8>, _>(row, idx, |b| json!(BASE64.encode(b))),
        _ => None,
    } {
        return v;
    }

    cell_to_json_untyped(row, idx, col_type)
}

fn array_cell_to_json(row: &tokio_postgres::Row, idx: usize, elem: &Type) -> Value {
    let try_array = |result: Result<Option<Vec<Value>>, tokio_postgres::Error>| -> Option<Value> {
        match result {
            Ok(Some(items)) => Some(Value::Array(items)),
            Ok(None) => Some(Value::Null),
            Err(_) => None,
        }
    };

    match *elem {
        Type::BOOL => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<bool>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|x| json!(x)).collect())),
            ) {
                return v;
            }
        }
        Type::INT2 => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<i16>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|x| json!(x)).collect())),
            ) {
                return v;
            }
        }
        Type::INT4 | Type::OID => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<i32>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|x| json!(x)).collect())),
            ) {
                return v;
            }
        }
        Type::INT8 => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<i64>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|x| json!(x)).collect())),
            ) {
                return v;
            }
        }
        Type::FLOAT4 => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<f32>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|n| json!(n)).collect())),
            ) {
                return v;
            }
        }
        Type::FLOAT8 => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<f64>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|n| json!(n)).collect())),
            ) {
                return v;
            }
        }
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<String>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(Value::String).collect())),
            ) {
                return v;
            }
        }
        Type::UUID => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<Uuid>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|u| json!(u.to_string())).collect())),
            ) {
                return v;
            }
        }
        Type::TIMESTAMP => {
            if let Some(v) = try_array(row.try_get::<_, Option<Vec<NaiveDateTime>>>(idx).map(|v| {
                v.map(|a| {
                    a.into_iter()
                        .map(|t| json!(t.format("%Y-%m-%dT%H:%M:%S%.f").to_string()))
                        .collect()
                })
            })) {
                return v;
            }
        }
        Type::TIMESTAMPTZ => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<DateTime<FixedOffset>>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|t| json!(t.to_rfc3339())).collect())),
            ) {
                return v;
            }
        }
        Type::DATE => {
            if let Some(v) = try_array(row.try_get::<_, Option<Vec<NaiveDate>>>(idx).map(|v| {
                v.map(|a| {
                    a.into_iter()
                        .map(|d| json!(d.format("%Y-%m-%d").to_string()))
                        .collect()
                })
            })) {
                return v;
            }
        }
        Type::JSON | Type::JSONB => {
            if let Some(v) = try_array(row.try_get::<_, Option<Vec<Value>>>(idx)) {
                return v;
            }
        }
        Type::NUMERIC => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<Decimal>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|d| json!(d.to_string())).collect())),
            ) {
                return v;
            }
        }
        Type::MONEY => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<i64>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|m| json!(money_to_string(m))).collect())),
            ) {
                return v;
            }
        }
        Type::BYTEA => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<Vec<u8>>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|b| json!(BASE64.encode(b))).collect())),
            ) {
                return v;
            }
        }
        _ => {}
    }

    cell_to_json_untyped(row, idx, row.columns()[idx].type_())
}

/// PostgreSQL `money` is int64 in ten-thousandths of the base currency unit.
fn money_to_string(v: i64) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let abs = v.unsigned_abs();
    format!("{}{}.{:04}", sign, abs / 10_000, abs % 10_000)
}

/// Last-resort decoding for unknown or composite Postgres types — never silent null for non-null cells.
fn cell_to_json_untyped(row: &tokio_postgres::Row, idx: usize, pg_type: &Type) -> Value {
    if let Ok(Some(s)) = row.try_get::<_, Option<String>>(idx) {
        return Value::String(s);
    }
    json!({
        "__untyped": true,
        "type": pg_type.name()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn columnarize_read_payload_reshapes_rows_and_drops_all_null_columns() {
        let payload = json!({
            "rows": [
                { "id": 1, "name": "a", "note": null },
                { "id": 2, "name": "b", "note": null },
            ],
            "truncated": true,
            "maxRows": 500,
        });
        let out = columnarize_read_payload(payload);
        assert_eq!(out["columns"], json!(["id", "name"]));
        assert_eq!(out["rows"], json!([[1, "a"], [2, "b"]]));
        assert_eq!(out["allNullColumns"], json!(["note"]));
        // Sibling keys survive the reshape untouched.
        assert_eq!(out["truncated"], json!(true));
        assert_eq!(out["maxRows"], json!(500));
    }

    #[test]
    fn columnarize_read_payload_empty_rows_is_noop_shape() {
        let payload = json!({ "rows": [] });
        let out = columnarize_read_payload(payload);
        assert_eq!(out["columns"], json!([]));
        assert_eq!(out["rows"], json!([]));
    }

    #[test]
    fn columnarize_row_arrays_reshapes_nested_uniform_arrays() {
        // Mirrors get_ddl's "table" branch shape: a top-level object with
        // several independent nested row arrays.
        let value = json!({
            "table": "public.orders",
            "columns": [
                { "column_name": "id", "data_type": "integer" },
                { "column_name": "status", "data_type": "text" },
            ],
            "constraints": [
                { "name": "orders_pkey", "definition": "PRIMARY KEY (id)" },
            ],
        });
        let out = columnarize_row_arrays(value);
        assert_eq!(
            out["columns"]["columns"],
            json!(["column_name", "data_type"])
        );
        assert_eq!(
            out["columns"]["rows"],
            json!([["id", "integer"], ["status", "text"]])
        );
        assert_eq!(out["constraints"]["columns"], json!(["definition", "name"]));
    }

    #[test]
    fn columnarize_row_arrays_leaves_heterogeneous_arrays_alone() {
        // Different key sets per element — not a result set, must not be
        // grid-ified (would silently drop/misalign fields).
        let value = json!({
            "items": [
                { "a": 1, "b": 2 },
                { "a": 1, "c": 3 },
            ]
        });
        let out = columnarize_row_arrays(value.clone());
        assert_eq!(out, value);
    }

    #[test]
    fn columnarize_row_arrays_leaves_primitive_and_empty_arrays_alone() {
        let value = json!({ "warnings": ["a", "b"], "empty": [] });
        let out = columnarize_row_arrays(value.clone());
        assert_eq!(out, value);
    }

    #[test]
    fn columnarize_row_arrays_is_idempotent_on_already_columnar_shape() {
        let value = json!({ "columns": ["n"], "rows": [[1], [2]] });
        let out = columnarize_row_arrays(value.clone());
        assert_eq!(out, value);
    }

    #[test]
    fn redact_pii_replaces_matching_columns() {
        let rows = vec![json!({"id": 1, "ssn": "123-45-6789"})];
        let tables = vec![ObjectRef::new("public", "users")];
        let pii = vec!["public.users.ssn".into()];
        let (out, cols) = redact_pii_in_rows(rows, &pii, &tables);
        assert_eq!(cols, vec!["ssn"]);
        assert_eq!(out[0]["ssn"], json!(PII_REDACTED));
        assert_eq!(out[0]["id"], json!(1));
    }
}
