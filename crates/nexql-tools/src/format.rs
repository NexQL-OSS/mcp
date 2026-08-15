// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Output format helpers for `run_select`.

use serde_json::Value;

pub fn escape_markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

pub fn value_to_display_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

/// Render columnar query output as a GitHub-flavored markdown pipe table.
pub fn rows_to_markdown(columns: &[String], rows: &[Vec<Value>]) -> String {
    if columns.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push('|');
    for col in columns {
        out.push(' ');
        out.push_str(&escape_markdown_cell(col));
        out.push_str(" |");
    }
    out.push('\n');
    out.push('|');
    for _ in columns {
        out.push_str(" --- |");
    }
    out.push('\n');
    for row in rows {
        out.push('|');
        for (idx, _) in columns.iter().enumerate() {
            let cell = row.get(idx).map(value_to_display_string).unwrap_or_default();
            out.push(' ');
            out.push_str(&escape_markdown_cell(&cell));
            out.push_str(" |");
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn markdown_table_renders_headers_and_rows() {
        let md = rows_to_markdown(
            &["id".into(), "name".into()],
            &[vec![json!(1), json!("alice")], vec![json!(2), json!("bob")]],
        );
        assert!(md.contains("| id | name |"));
        assert!(md.contains("| 1 | alice |"));
        assert!(md.contains("| 2 | bob |"));
    }

    #[test]
    fn markdown_escapes_pipes() {
        let md = rows_to_markdown(&["x".into()], &[vec![json!("a|b")]]);
        assert!(md.contains(r"a\|b"));
    }
}
