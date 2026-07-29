//! Structural object hash — port of `pro/src/features/dbindex/objectHash.ts`.
//!
//! SHA-1 hex digest of selected ObjectEntry fields; used for shard skip and
//! embedding refresh decisions.

use sha1::{Digest, Sha1};

use crate::model::ObjectEntry;

/// SHA-1 hex of structural characteristics — matches TS `computeObjectHash`.
pub fn compute_object_hash(entry: &ObjectEntry) -> String {
    let mut parts: Vec<String> = vec![
        kind_wire(entry.kind),
        // TS uses `String(entry.rowEstimate || 0)` — integer-ish for whole numbers.
        format_row_estimate(entry.row_estimate),
        entry.comment.clone().unwrap_or_default(),
    ];

    if !entry.columns.is_empty() {
        let col_strings: Vec<String> = entry
            .columns
            .iter()
            .map(|c| {
                format!(
                    "{}:{}:{}:{}:{}:{}",
                    c.name,
                    c.type_name,
                    if c.not_null { "notnull" } else { "nullable" },
                    c.default_value.as_deref().unwrap_or(""),
                    c.comment.as_deref().unwrap_or(""),
                    c.ordinal
                )
            })
            .collect();
        parts.push(format!("cols:{}", col_strings.join("|")));
    }

    if let Some(ref pk) = entry.primary_key {
        parts.push(format!("pk:{}", pk.join(",")));
    }

    if let Some(ref fks) = entry.foreign_keys {
        let mut fk_strings: Vec<String> = fks
            .iter()
            .map(|fk| {
                format!(
                    "{}:{}:{}:{}",
                    fk.name,
                    fk.columns.join(","),
                    fk.ref_table,
                    fk.ref_columns.join(",")
                )
            })
            .collect();
        fk_strings.sort();
        parts.push(format!("fks:{}", fk_strings.join("|")));
    }

    if let Some(ref indexes) = entry.indexes {
        let mut idx_strings: Vec<String> = indexes
            .iter()
            .map(|idx| {
                let partial = idx
                    .partial
                    .as_ref()
                    .and_then(|p| p.as_ref())
                    .map(|s| s.as_str())
                    .unwrap_or("");
                format!(
                    "{}:{}:{}:{}:{}",
                    idx.name,
                    idx.columns.join(","),
                    idx.unique,
                    idx.method,
                    partial
                )
            })
            .collect();
        idx_strings.sort();
        parts.push(format!("idx:{}", idx_strings.join("|")));
    }

    if let Some(ref checks) = entry.checks {
        let mut check_strings: Vec<String> = checks
            .iter()
            .map(|ck| format!("{}:{}", ck.name, ck.expr))
            .collect();
        check_strings.sort();
        parts.push(format!("checks:{}", check_strings.join("|")));
    }

    if let Some(ref def) = entry.definition {
        parts.push(format!("def:{def}"));
    }

    if let Some(ref sig) = entry.signature {
        parts.push(format!(
            "sig:{}:{}:{}",
            sig,
            entry.language.as_deref().unwrap_or(""),
            entry.volatility.as_deref().unwrap_or("")
        ));
    }

    if let Some(ref values) = entry.values {
        parts.push(format!("vals:{}", values.join(",")));
    }

    if let Some(ref base) = entry.base_type {
        parts.push(format!(
            "base:{}:{}",
            base,
            entry.constraint.as_deref().unwrap_or("")
        ));
    }

    let combined = parts.join(";;");
    hex_sha1(combined.as_bytes())
}

/// Hash used for shard content identity — matches TS
/// `computeObjectHash({ definition: content })`.
///
/// Wire shape: `['', '0', '', 'def:' + content].join(';;')` → `;;0;;;;def:…`
pub fn compute_definition_hash(content: &str) -> String {
    let combined = format!(";;0;;;;def:{content}");
    hex_sha1(combined.as_bytes())
}

fn kind_wire(kind: crate::model::DbObjectKind) -> String {
    use crate::model::DbObjectKind::*;
    match kind {
        Table => "table",
        View => "view",
        Matview => "matview",
        Function => "function",
        Enum => "enum",
        Domain => "domain",
        Sequence => "sequence",
    }
    .to_owned()
}

fn format_row_estimate(v: f64) -> String {
    // TS `String(entry.rowEstimate || 0)` — Number to string; whole numbers without decimal.
    if v == 0.0 {
        return "0".into();
    }
    if v.fract() == 0.0 && v.abs() < (i64::MAX as f64) {
        return format!("{}", v as i64);
    }
    // Match JS number stringification for non-integers.
    let s = v.to_string();
    if s == "0" { "0".into() } else { s }
}

fn hex_sha1(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ColumnEntry, DbObjectKind};

    #[test]
    fn definition_only_hash_shape() {
        let h = compute_definition_hash(r#"{"a":1}"#);
        assert_eq!(h.len(), 40);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn object_hash_stable() {
        let entry = ObjectEntry {
            kind: DbObjectKind::Table,
            oid: 1,
            object_hash: String::new(),
            comment: Some("users".into()),
            row_estimate: 10.0,
            size_bytes: 0,
            columns: vec![ColumnEntry {
                name: "id".into(),
                type_name: "integer".into(),
                not_null: true,
                default_value: None,
                comment: None,
                ordinal: 1,
                is_pk: None,
                profile: None,
                pii: None,
            }],
            primary_key: Some(vec!["id".into()]),
            foreign_keys: None,
            indexes: None,
            checks: None,
            excluded: None,
            definition: None,
            signature: None,
            language: None,
            volatility: None,
            body: None,
            values: None,
            base_type: None,
            constraint: None,
        };
        let a = compute_object_hash(&entry);
        let b = compute_object_hash(&entry);
        assert_eq!(a, b);
        assert_eq!(a.len(), 40);
    }
}
