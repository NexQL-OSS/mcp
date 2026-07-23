//! Catalog SQL ported verbatim from `pro/src/features/dbindex/catalogQueries.ts`.
//! Phase 0 only needs the first three queries (relations, columns, constraints/FKs).

/// Fetch relations (tables, views, matviews, foreign tables, partitioned tables).
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
"#;

/// Fetch columns for multiple relations by OID.
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

/// Fetch constraints for multiple relations by OID.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_queries_mention_required_catalogs() {
        assert!(RELATIONS_QUERY.contains("pg_class"));
        assert!(COLUMNS_QUERY.contains("pg_attribute"));
        assert!(CONSTRAINTS_QUERY.contains("pg_constraint"));
    }
}
