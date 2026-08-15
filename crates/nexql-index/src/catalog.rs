// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Catalog SQL query constants — port of
//! `pro/src/features/dbindex/catalogQueries.ts`.
//!
//! Strings match the TypeScript source verbatim (including leading/trailing
//! newlines) so the builder can stay query-identical for golden-file parity.

use serde::Deserialize;

use crate::model::DbObjectKind;

// ---------------------------------------------------------------------------
// Raw row shapes (snake_case keys match SQL aliases)
// ---------------------------------------------------------------------------

/// Row from [`RELATIONS_QUERY`].
#[derive(Debug, Clone, Deserialize)]
pub struct RawRelationRow {
    pub oid: i32,
    pub schema_name: String,
    pub name: String,
    pub kind: String,
    pub comment: Option<String>,
    /// Postgres may return bigint as number or string depending on driver.
    pub row_estimate: serde_json::Value,
    pub size_bytes: serde_json::Value,
}

/// Row from [`COLUMNS_QUERY`].
#[derive(Debug, Clone, Deserialize)]
pub struct RawColumnRow {
    pub table_oid: i32,
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub not_null: bool,
    pub default_value: Option<String>,
    pub comment: Option<String>,
    pub ordinal: i32,
}

/// Row from [`CONSTRAINTS_QUERY`].
#[derive(Debug, Clone, Deserialize)]
pub struct RawConstraintRow {
    pub table_oid: i32,
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub definition: String,
    pub ref_table_oid: Option<i32>,
    pub key_positions: Option<Vec<i32>>,
    pub ref_key_positions: Option<Vec<i32>>,
}

/// Row from [`INDEXES_QUERY`].
#[derive(Debug, Clone, Deserialize)]
pub struct RawIndexRow {
    pub table_oid: i32,
    pub name: String,
    pub unique: bool,
    pub method: String,
    pub definition: String,
    pub key_positions: Option<Vec<i32>>,
}

/// Row from [`VIEW_DEFINITIONS_QUERY`].
#[derive(Debug, Clone, Deserialize)]
pub struct RawViewRow {
    pub oid: i32,
    pub definition: String,
}

/// Row from [`FUNCTIONS_QUERY`].
#[derive(Debug, Clone, Deserialize)]
pub struct RawFunctionRow {
    pub oid: i32,
    pub schema_name: String,
    pub name: String,
    pub arguments: String,
    /// `None` for procedures (`prokind = 'p'`) — `pg_get_function_result()` is NULL.
    pub result_type: Option<String>,
    pub language: String,
    pub volatility: String,
    pub body: String,
    pub comment: Option<String>,
}

/// Row from [`ENUMS_QUERY`].
#[derive(Debug, Clone, Deserialize)]
pub struct RawEnumRow {
    pub oid: i32,
    pub schema_name: String,
    pub name: String,
    pub value: String,
    pub sort_order: i32,
}

/// Row from [`DOMAINS_QUERY`].
#[derive(Debug, Clone, Deserialize)]
pub struct RawDomainRow {
    pub oid: i32,
    pub schema_name: String,
    pub name: String,
    pub base_type: String,
    pub constraint_name: Option<String>,
    pub constraint_definition: Option<String>,
}

// ---------------------------------------------------------------------------
// SQL constants (verbatim from catalogQueries.ts)
// ---------------------------------------------------------------------------

/// 1. Fetch relations (tables, views, materialized views, foreign tables, partitioned tables)
pub const RELATIONS_QUERY: &str = r#"
SELECT
  c.oid::integer AS oid,
  n.nspname AS schema_name,
  c.relname AS name,
  c.relkind AS kind,
  d.description AS comment,
  c.reltuples::bigint AS row_estimate,
  pg_total_relation_size(c.oid)::bigint AS size_bytes
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
LEFT JOIN pg_description d ON d.objoid = c.oid AND d.objsubid = 0
WHERE n.nspname = ANY($1)
  AND c.relkind IN ('r', 'v', 'f', 'm', 'p')
  AND NOT c.relispartition
"#;

/// 2. Fetch columns for multiple relations by OID
pub const COLUMNS_QUERY: &str = r#"
SELECT
  a.attrelid::integer AS table_oid,
  a.attname AS name,
  format_type(a.atttypid, a.atttypmod) AS type,
  a.attnotnull AS not_null,
  pg_get_expr(ad.adbin, ad.adrelid) AS default_value,
  d.description AS comment,
  a.attnum AS ordinal
FROM pg_attribute a
LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum
LEFT JOIN pg_description d ON d.objoid = a.attrelid AND d.objsubid = a.attnum
WHERE a.attnum > 0
  AND NOT a.attisdropped
  AND a.attrelid = ANY($1)
ORDER BY a.attrelid, a.attnum
"#;

/// 3. Fetch constraints for multiple relations by OID
pub const CONSTRAINTS_QUERY: &str = r#"
SELECT
  con.conrelid::integer AS table_oid,
  con.conname AS name,
  con.contype AS type,
  pg_get_constraintdef(con.oid) AS definition,
  con.confrelid::integer AS ref_table_oid,
  con.conkey::integer[] AS key_positions,
  con.confkey::integer[] AS ref_key_positions
FROM pg_constraint con
WHERE con.conrelid = ANY($1)
"#;

/// 4. Fetch indexes for multiple relations by OID (excluding constraint indexes that are PK/Unique to reduce noise)
pub const INDEXES_QUERY: &str = r#"
SELECT
  ind.indrelid::integer AS table_oid,
  c.relname AS name,
  ind.indisunique AS unique,
  am.amname AS method,
  pg_get_indexdef(ind.indexrelid) AS definition,
  ind.indkey::integer[] AS key_positions
FROM pg_index ind
JOIN pg_class c ON c.oid = ind.indexrelid
JOIN pg_class tc ON tc.oid = ind.indrelid
JOIN pg_am am ON am.oid = c.relam
WHERE ind.indrelid = ANY($1)
"#;

/// 5. Fetch definitions for views and materialized views
pub const VIEW_DEFINITIONS_QUERY: &str = r#"
SELECT
  c.oid::integer AS oid,
  pg_get_viewdef(c.oid) AS definition
FROM pg_class c
WHERE c.relkind IN ('v', 'm')
  AND c.oid = ANY($1)
"#;

/// 6. Fetch functions in schemas
pub const FUNCTIONS_QUERY: &str = r#"
SELECT
  p.oid::integer AS oid,
  n.nspname AS schema_name,
  p.proname AS name,
  pg_get_function_arguments(p.oid) AS arguments,
  pg_get_function_result(p.oid) AS result_type,
  l.lanname AS language,
  p.provolatile AS volatility,
  p.prosrc AS body,
  d.description AS comment
FROM pg_proc p
JOIN pg_namespace n ON n.oid = p.pronamespace
JOIN pg_language l ON l.oid = p.prolang
LEFT JOIN pg_description d ON d.objoid = p.oid
WHERE n.nspname = ANY($1)
"#;

/// 7. Fetch enums in schemas
pub const ENUMS_QUERY: &str = r#"
SELECT
  t.oid::integer AS oid,
  n.nspname AS schema_name,
  t.typname AS name,
  e.enumlabel AS value,
  e.enumsortorder::integer AS sort_order
FROM pg_enum e
JOIN pg_type t ON t.oid = e.enumtypid
JOIN pg_namespace n ON n.oid = t.typnamespace
WHERE n.nspname = ANY($1)
ORDER BY t.oid, e.enumsortorder
"#;

/// 8. Fetch domains in schemas
pub const DOMAINS_QUERY: &str = r#"
SELECT
  t.oid::integer AS oid,
  n.nspname AS schema_name,
  t.typname AS name,
  format_type(t.typbasetype, t.typtypmod) AS base_type,
  con.conname AS constraint_name,
  pg_get_constraintdef(con.oid) AS constraint_definition
FROM pg_type t
JOIN pg_namespace n ON n.oid = t.typnamespace
LEFT JOIN pg_constraint con ON con.contypid = t.oid
WHERE n.nspname = ANY($1)
  AND t.typtype = 'd'
"#;

/// 9. Schema fingerprint — must stay in sync with `IndexManifest.schemaFingerprint` format.
///
/// Differs from SchemaPoller's fingerprint; staleness checks against a manifest
/// must use this query, never the poller's.
pub const SCHEMA_FINGERPRINT_QUERY: &str = r#"
SELECT
  COUNT(*)::text                                    AS object_count,
  COALESCE(MAX(c.oid)::text, '0')                   AS max_oid,
  COALESCE(SUM(c.reltuples)::bigint::text, '0')     AS total_rows_estimate,
  (SELECT COUNT(*)::text FROM pg_namespace
   WHERE nspname NOT IN ('pg_catalog','information_schema','pg_toast')
     AND nspname NOT LIKE 'pg_%')                   AS schema_count,
  COALESCE((SELECT MAX(oid)::text FROM pg_namespace
            WHERE nspname NOT IN ('pg_catalog','information_schema','pg_toast')
              AND nspname NOT LIKE 'pg_%'), '0')     AS max_schema_oid
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
  AND c.relkind IN ('r', 'v', 'f', 'm', 'p')
"#;

/// 10. Non-system schemas in a database
pub const NON_SYSTEM_SCHEMAS_QUERY: &str = r#"
SELECT nspname
FROM pg_namespace
WHERE nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
  AND nspname NOT LIKE 'pg_%'
ORDER BY nspname
"#;

/// Map `pg_class.relkind` to [`DbObjectKind`] — matches TS `mapRelkindToDbObjectKind`.
pub fn map_relkind_to_db_object_kind(relkind: &str) -> DbObjectKind {
    match relkind {
        "r" => DbObjectKind::Table,
        "v" => DbObjectKind::View,
        "m" => DbObjectKind::Matview,
        "f" => DbObjectKind::Table, // foreign tables → tables for NL grounding
        "p" => DbObjectKind::Table, // partitioned tables → tables
        _ => DbObjectKind::Table,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_queries_non_empty() {
        let queries = [
            ("RELATIONS_QUERY", RELATIONS_QUERY),
            ("COLUMNS_QUERY", COLUMNS_QUERY),
            ("CONSTRAINTS_QUERY", CONSTRAINTS_QUERY),
            ("INDEXES_QUERY", INDEXES_QUERY),
            ("VIEW_DEFINITIONS_QUERY", VIEW_DEFINITIONS_QUERY),
            ("FUNCTIONS_QUERY", FUNCTIONS_QUERY),
            ("ENUMS_QUERY", ENUMS_QUERY),
            ("DOMAINS_QUERY", DOMAINS_QUERY),
            ("SCHEMA_FINGERPRINT_QUERY", SCHEMA_FINGERPRINT_QUERY),
            ("NON_SYSTEM_SCHEMAS_QUERY", NON_SYSTEM_SCHEMAS_QUERY),
        ];
        for (name, sql) in queries {
            assert!(!sql.trim().is_empty(), "{name} must be non-empty");
        }
    }

    #[test]
    fn key_query_fragments_present() {
        assert!(RELATIONS_QUERY.contains("pg_class"));
        assert!(RELATIONS_QUERY.contains("relkind IN ('r', 'v', 'f', 'm', 'p')"));
        assert!(COLUMNS_QUERY.contains("pg_attribute"));
        assert!(COLUMNS_QUERY.contains("format_type"));
        assert!(CONSTRAINTS_QUERY.contains("pg_constraint"));
        assert!(INDEXES_QUERY.contains("pg_index"));
        assert!(VIEW_DEFINITIONS_QUERY.contains("pg_get_viewdef"));
        assert!(FUNCTIONS_QUERY.contains("pg_proc"));
        assert!(ENUMS_QUERY.contains("pg_enum"));
        assert!(DOMAINS_QUERY.contains("typtype = 'd'"));
        assert!(SCHEMA_FINGERPRINT_QUERY.contains("object_count"));
        assert!(SCHEMA_FINGERPRINT_QUERY.contains("max_schema_oid"));
        assert!(NON_SYSTEM_SCHEMAS_QUERY.contains("NOT LIKE 'pg_%'"));
    }

    #[test]
    fn map_relkind_matches_ts() {
        assert_eq!(map_relkind_to_db_object_kind("r"), DbObjectKind::Table);
        assert_eq!(map_relkind_to_db_object_kind("v"), DbObjectKind::View);
        assert_eq!(map_relkind_to_db_object_kind("m"), DbObjectKind::Matview);
        assert_eq!(map_relkind_to_db_object_kind("f"), DbObjectKind::Table);
        assert_eq!(map_relkind_to_db_object_kind("p"), DbObjectKind::Table);
        assert_eq!(map_relkind_to_db_object_kind("?"), DbObjectKind::Table);
    }
}
