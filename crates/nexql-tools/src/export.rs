// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Result formatting for `export_query` (CSV / JSON / SQL-INSERT).
//! Ported from core `CoreHandlers.rowsToCsv` / `rowsToSqlInsert`.

use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
    SqlInsert,
}

impl ExportFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "csv" => Some(Self::Csv),
            "json" => Some(Self::Json),
            "sqlinsert" | "sql_insert" | "insert" => Some(Self::SqlInsert),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            Self::SqlInsert => "sqlinsert",
        }
    }
}

/// Extract column order from the first row object; empty if no rows.
pub fn columns_from_rows(rows: &[Value]) -> Vec<String> {
    rows.first()
        .and_then(|r| r.as_object())
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default()
}

pub fn rows_to_csv(rows: &[Value], columns: &[String]) -> String {
    let mut out = String::new();
    out.push_str(
        &columns
            .iter()
            .map(|c| csv_quote(c))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push('\n');
    for row in rows {
        let obj = row.as_object();
        let line = columns
            .iter()
            .map(|col| {
                let val = obj.and_then(|m| m.get(col)).unwrap_or(&Value::Null);
                csv_cell(val)
            })
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn csv_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn csv_cell(val: &Value) -> String {
    if val.is_null() {
        return String::new();
    }
    let str = match val {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    };
    if str.contains(',') || str.contains('\n') || str.contains('"') {
        csv_quote(&str)
    } else {
        str
    }
}

pub fn rows_to_sql_insert(rows: &[Value], columns: &[String], schema: &str, table: &str) -> String {
    let table_name = format!(
        "\"{}\".\"{}\"",
        schema.replace('"', "\"\""),
        table.replace('"', "\"\"")
    );
    let cols = columns
        .iter()
        .map(|c| format!("\"{}\"", c.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(", ");

    rows.iter()
        .map(|row| {
            let obj = row.as_object().cloned().unwrap_or_else(Map::new);
            let values = columns
                .iter()
                .map(|col| sql_literal(obj.get(col).unwrap_or(&Value::Null)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("INSERT INTO {table_name} ({cols}) VALUES ({values});")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sql_literal(val: &Value) -> String {
    match val {
        Value::Null => "NULL".into(),
        Value::Bool(b) => {
            if *b {
                "TRUE".into()
            } else {
                "FALSE".into()
            }
        }
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        other => format!("'{}'", other.to_string().replace('\'', "''")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn csv_escapes_quotes_and_commas() {
        let rows = [json!({ "a": "hello, world", "b": "say \"hi\"" })];
        let cols = vec!["a".into(), "b".into()];
        let csv = rows_to_csv(&rows, &cols);
        assert!(csv.contains("\"hello, world\""));
        assert!(csv.contains("\"say \"\"hi\"\"\""));
    }

    #[test]
    fn sql_insert_null_bool_number() {
        let rows = [json!({ "id": 1, "ok": true, "note": null })];
        let cols = vec!["id".into(), "ok".into(), "note".into()];
        let sql = rows_to_sql_insert(&rows, &cols, "public", "t");
        assert_eq!(
            sql,
            "INSERT INTO \"public\".\"t\" (\"id\", \"ok\", \"note\") VALUES (1, TRUE, NULL);"
        );
    }

    #[test]
    fn format_parse() {
        assert_eq!(ExportFormat::parse("CSV"), Some(ExportFormat::Csv));
        assert_eq!(
            ExportFormat::parse("sql_insert"),
            Some(ExportFormat::SqlInsert)
        );
        assert!(ExportFormat::parse("xlsx").is_none());
    }
}
