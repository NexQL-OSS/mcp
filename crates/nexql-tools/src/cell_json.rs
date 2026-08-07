// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Typed Postgres cell → JSON conversion shared by read and write tools.

use nexql_policy::{ObjectRef, PII_REDACTED, column_matches_pii_policy};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
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
