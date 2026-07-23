//! SQL builders for Phase 4 monitoring / DDL tools.
//! Ported from `pro/.../ToolExecutor.ts` + `core/commands/sql/{profile,monitoring}.ts`.

/// Max rows for `slow_queries` (matches TS `MonitoringSQL.slowQueries`).
pub const SLOW_QUERIES_MAX: u32 = 50;
pub const SLOW_QUERIES_DEFAULT: u32 = 10;

/// Validate `schema.name` (or bare name → `public`) as plain SQL identifiers.
pub fn parse_ref(ref_: &str) -> Result<(String, String), String> {
    let trimmed = ref_.trim();
    if trimmed.is_empty() {
        return Err("Ref parameter is required".into());
    }
    let parts: Vec<&str> = trimmed.split('.').collect();
    let (schema, name) = match parts.as_slice() {
        [name] => ("public", *name),
        [schema, name] => (*schema, *name),
        _ => {
            return Err(format!(
                "Invalid object reference \"{ref_}\". Expected format \"schema.name\" with plain identifiers."
            ));
        }
    };
    if !is_safe_ident(schema) || !is_safe_ident(name) {
        return Err(format!(
            "Invalid object reference \"{ref_}\". Expected format \"schema.name\" with plain identifiers."
        ));
    }
    Ok((schema.to_owned(), name.to_owned()))
}

fn is_safe_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Double-quote an identifier (idents already validated via [`parse_ref`]).
pub fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

pub fn quote_ref(schema: &str, name: &str) -> String {
    format!("{}.{}", quote_ident(schema), quote_ident(name))
}

/// String literal for `'\"schema\".\"name\"'::regclass`.
pub fn regclass_literal(schema: &str, name: &str) -> String {
    format!("'{}'::regclass", quote_ref(schema, name).replace('\'', "''"))
}

pub fn table_stats(schema: &str, table: &str) -> String {
    format!(
        r#"
SELECT
  schemaname,
  relname AS table_name,
  n_live_tup AS approximate_row_count,
  pg_size_pretty(pg_total_relation_size(quote_ident(schemaname) || '.' || quote_ident(relname))) AS total_size,
  pg_size_pretty(pg_relation_size(quote_ident(schemaname) || '.' || quote_ident(relname))) AS table_size,
  pg_size_pretty(pg_indexes_size(quote_ident(schemaname) || '.' || quote_ident(relname))) AS indexes_size,
  pg_size_pretty(pg_total_relation_size(quote_ident(schemaname) || '.' || quote_ident(relname)) -
                 pg_relation_size(quote_ident(schemaname) || '.' || quote_ident(relname)) -
                 pg_indexes_size(quote_ident(schemaname) || '.' || quote_ident(relname))) AS toast_size
FROM pg_stat_user_tables
WHERE schemaname = '{schema}' AND relname = '{table}'
"#
    )
    .trim()
    .to_owned()
}

pub fn column_stats(schema: &str, table: &str) -> String {
    format!(
        r#"
SELECT
  attname AS column_name,
  null_frac AS null_fraction,
  n_distinct AS distinct_values,
  avg_width AS avg_bytes,
  correlation,
  most_common_vals::text AS most_common_values,
  most_common_freqs::text AS frequencies
FROM pg_stats
WHERE schemaname = '{schema}' AND tablename = '{table}'
ORDER BY attname
"#
    )
    .trim()
    .to_owned()
}

pub fn column_details(schema: &str, table: &str) -> String {
    let reg = regclass_literal(schema, table);
    format!(
        r#"
SELECT
  a.attname AS column_name,
  pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
  a.attnotnull AS not_null,
  COALESCE(pg_get_expr(ad.adbin, ad.adrelid), '') AS default_value,
  CASE
    WHEN a.attnum = ANY(pk.conkey) THEN 'PK'
    WHEN a.attnum = ANY(uk.conkey) THEN 'UNIQUE'
    ELSE ''
  END AS key_type
FROM pg_catalog.pg_attribute a
LEFT JOIN pg_catalog.pg_attrdef ad ON (a.attrelid = ad.adrelid AND a.attnum = ad.adnum)
LEFT JOIN pg_catalog.pg_constraint pk ON (pk.conrelid = a.attrelid AND pk.contype = 'p')
LEFT JOIN pg_catalog.pg_constraint uk ON (uk.conrelid = a.attrelid AND uk.contype = 'u' AND a.attnum = ANY(uk.conkey))
WHERE a.attrelid = {reg}
  AND a.attnum > 0
  AND NOT a.attisdropped
ORDER BY a.attnum
"#
    )
    .trim()
    .to_owned()
}

pub fn table_activity(schema: &str, table: &str) -> String {
    format!(
        r#"
SELECT
  seq_scan AS sequential_scans,
  seq_tup_read AS rows_seq_read,
  idx_scan AS index_scans,
  idx_tup_fetch AS rows_idx_fetched,
  n_tup_ins AS rows_inserted,
  n_tup_upd AS rows_updated,
  n_tup_del AS rows_deleted,
  n_tup_hot_upd AS hot_updates,
  n_live_tup AS live_rows,
  n_dead_tup AS dead_rows,
  last_vacuum,
  last_autovacuum,
  last_analyze,
  last_autoanalyze,
  vacuum_count,
  autovacuum_count,
  analyze_count,
  autoanalyze_count
FROM pg_stat_user_tables
WHERE schemaname = '{schema}' AND relname = '{table}'
"#
    )
    .trim()
    .to_owned()
}

pub fn index_usage(schema: &str, table: &str) -> String {
    format!(
        r#"
SELECT
  s.indexrelname AS index_name,
  pg_size_pretty(pg_relation_size(s.indexrelid)) AS index_size,
  s.idx_scan AS number_of_scans,
  s.idx_tup_read AS tuples_read,
  s.idx_tup_fetch AS tuples_fetched,
  pg_get_indexdef(s.indexrelid) AS index_definition,
  CASE
    WHEN i.indisunique THEN 'UNIQUE'
    WHEN i.indisprimary THEN 'PRIMARY KEY'
    ELSE 'INDEX'
  END AS index_type
FROM pg_stat_user_indexes s
JOIN pg_index i ON s.indexrelid = i.indexrelid
WHERE s.schemaname = '{schema}' AND s.relname = '{table}'
ORDER BY s.idx_scan DESC
"#
    )
    .trim()
    .to_owned()
}

pub fn running_queries() -> &'static str {
    r#"
SELECT pid,
       usename AS user,
       datname AS database,
       state,
       wait_event_type,
       wait_event,
       (now() - query_start)::text AS duration,
       query_start,
       LEFT(query, 500) AS query
FROM pg_stat_activity
WHERE pid != pg_backend_pid()
  AND state IS DISTINCT FROM 'idle'
  AND datname = current_database()
ORDER BY query_start ASC
LIMIT 100
"#
    .trim()
}

pub fn blocking_locks() -> &'static str {
    r#"
SELECT
    blocked_locks.pid     AS blocked_pid,
    blocked_activity.usename  AS blocked_user,
    blocking_locks.pid     AS blocking_pid,
    blocking_activity.usename AS blocking_user,
    blocked_activity.query    AS blocked_query,
    blocking_activity.query   AS blocking_query,
    blocked_locks.mode        AS lock_mode,
    COALESCE(c.relname, 'null') AS locked_object
FROM  pg_catalog.pg_locks         blocked_locks
JOIN pg_catalog.pg_stat_activity blocked_activity  ON blocked_activity.pid = blocked_locks.pid
JOIN pg_catalog.pg_locks         blocking_locks
    ON blocking_locks.locktype = blocked_locks.locktype
    AND blocking_locks.database IS NOT DISTINCT FROM blocked_locks.database
    AND blocking_locks.relation IS NOT DISTINCT FROM blocked_locks.relation
    AND blocking_locks.page IS NOT DISTINCT FROM blocked_locks.page
    AND blocking_locks.tuple IS NOT DISTINCT FROM blocked_locks.tuple
    AND blocking_locks.virtualxid IS NOT DISTINCT FROM blocked_locks.virtualxid
    AND blocking_locks.transactionid IS NOT DISTINCT FROM blocked_locks.transactionid
    AND blocking_locks.classid IS NOT DISTINCT FROM blocked_locks.classid
    AND blocking_locks.objid IS NOT DISTINCT FROM blocked_locks.objid
    AND blocking_locks.objsubid IS NOT DISTINCT FROM blocked_locks.objsubid
    AND blocking_locks.pid != blocked_locks.pid
JOIN pg_catalog.pg_stat_activity blocking_activity ON blocking_activity.pid = blocking_locks.pid
LEFT JOIN pg_catalog.pg_class c ON c.oid = blocked_locks.relation
WHERE NOT blocked_locks.granted
AND blocked_activity.datname = current_database()
AND blocking_activity.datname = current_database()
"#
    .trim()
}

pub fn connection_states() -> &'static str {
    r#"
SELECT state, wait_event_type IS NOT NULL as waiting, count(*) as count
FROM pg_stat_activity
WHERE datname = current_database()
GROUP BY state, waiting
"#
    .trim()
}

pub fn cache_hit_ratio() -> &'static str {
    r#"
SELECT
  blks_hit,
  blks_read,
  CASE WHEN blks_hit + blks_read = 0 THEN NULL
       ELSE ROUND(blks_hit::numeric / (blks_hit + blks_read), 4)
  END AS cache_hit_ratio,
  xact_commit,
  xact_rollback,
  deadlocks,
  temp_files,
  temp_bytes
FROM pg_stat_database
WHERE datname = current_database()
"#
    .trim()
}

pub fn slow_queries(limit: u32) -> String {
    let capped = limit.clamp(1, SLOW_QUERIES_MAX);
    format!(
        r#"
SELECT
  queryid::text,
  LEFT(query, 500) AS query,
  calls,
  ROUND(mean_exec_time::numeric, 2)   AS mean_ms,
  ROUND(total_exec_time::numeric, 2)  AS total_ms,
  ROUND(stddev_exec_time::numeric, 2) AS stddev_ms,
  rows
FROM pg_stat_statements
WHERE query NOT LIKE '%pg_stat_statements%'
  AND query NOT LIKE 'BEGIN%'
  AND query NOT LIKE 'COMMIT%'
  AND query NOT LIKE 'ROLLBACK%'
  AND calls >= 5
ORDER BY mean_exec_time DESC
LIMIT {capped}
"#
    )
    .trim()
    .to_owned()
}

pub fn database_stats() -> &'static str {
    r#"
SELECT
    d.datname as "Database",
    pg_size_pretty(pg_database_size(d.datname)) as "Size",
    u.usename as "Owner",
    (SELECT count(*) FROM pg_stat_activity WHERE datname = current_database()) as "Active Connections",
    (SELECT count(*) FROM pg_namespace WHERE nspname NOT IN ('pg_catalog', 'information_schema')) as "Schemas",
    (SELECT count(*) FROM pg_tables WHERE schemaname NOT IN ('pg_catalog', 'information_schema')) as "Tables",
    (SELECT count(*) FROM pg_roles) as "Roles"
FROM pg_database d
JOIN pg_user u ON d.datdba = u.usesysid
WHERE d.datname = current_database()
"#
    .trim()
}

pub fn database_maintenance_stats() -> &'static str {
    r#"
SELECT
    schemaname || '.' || relname as "Table",
    n_dead_tup as "Dead Tuples",
    n_live_tup as "Live Tuples",
    last_vacuum as "Last Vacuum",
    last_autovacuum as "Last Auto Vacuum",
    pg_size_pretty(pg_total_relation_size(schemaname || '.' || relname)) as "Total Size"
FROM pg_stat_user_tables
WHERE n_dead_tup > 0
ORDER BY n_dead_tup DESC
LIMIT 20
"#
    .trim()
}

pub fn list_extensions() -> &'static str {
    r#"
SELECT e.extname AS name,
       e.extversion AS version,
       n.nspname AS schema
FROM pg_extension e
JOIN pg_namespace n ON n.oid = e.extnamespace
ORDER BY e.extname
"#
    .trim()
}

pub fn server_settings() -> &'static str {
    r#"
SELECT name, setting, unit, category, short_desc
FROM pg_settings
WHERE name IN (
  'max_connections', 'shared_buffers', 'work_mem', 'maintenance_work_mem',
  'effective_cache_size', 'random_page_cost', 'seq_page_cost',
  'statement_timeout', 'idle_in_transaction_session_timeout',
  'max_wal_size', 'checkpoint_completion_target', 'wal_buffers',
  'default_statistics_target', 'autovacuum', 'server_version',
  'shared_preload_libraries', 'default_transaction_read_only'
)
ORDER BY name
"#
    .trim()
}

/// Max rows for advisory reports (`suggest_indexes`, unused indexes, bloat, missing FKs).
pub const REPORT_LIMIT_MAX: u32 = 50;
pub const REPORT_LIMIT_DEFAULT: u32 = 20;

fn clamp_report_limit(limit: u32) -> u32 {
    limit.clamp(1, REPORT_LIMIT_MAX)
}

/// Tables with high sequential-scan ratio — primary `suggest_indexes` heuristic
/// (ported from Pro dashboard `highSeqScanTables`).
pub fn high_seq_scan_tables(limit: u32) -> String {
    let capped = clamp_report_limit(limit);
    format!(
        r#"
SELECT schemaname || '.' || relname AS table_name,
       seq_scan,
       COALESCE(idx_scan, 0) AS idx_scan,
       CASE WHEN seq_scan + COALESCE(idx_scan, 0) > 0
            THEN ROUND(100.0 * seq_scan / (seq_scan + COALESCE(idx_scan, 0)), 1)
            ELSE 0 END AS seq_scan_pct,
       n_live_tup AS row_count,
       'High sequential scan ratio — consider indexes on frequently filtered/joined columns' AS rationale
FROM pg_stat_user_tables
WHERE schemaname NOT IN ('pg_catalog', 'information_schema')
  AND seq_scan + COALESCE(idx_scan, 0) > 100
ORDER BY seq_scan_pct DESC, seq_scan DESC
LIMIT {capped}
"#
    )
    .trim()
    .to_owned()
}

/// Foreign-key columns lacking a covering btree index — classic missing-index heuristic.
pub fn unindexed_fk_columns(limit: u32) -> String {
    let capped = clamp_report_limit(limit);
    format!(
        r#"
SELECT
  n.nspname || '.' || c.relname AS table_name,
  a.attname AS column_name,
  confrelid::regclass::text AS references_table,
  'FK column without supporting index — CREATE INDEX ON ' ||
    quote_ident(n.nspname) || '.' || quote_ident(c.relname) ||
    ' (' || quote_ident(a.attname) || ')' AS suggestion
FROM pg_constraint con
JOIN pg_class c ON c.oid = con.conrelid
JOIN pg_namespace n ON n.oid = c.relnamespace
JOIN LATERAL unnest(con.conkey) WITH ORDINALITY AS ck(attnum, ord) ON true
JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ck.attnum
WHERE con.contype = 'f'
  AND n.nspname NOT IN ('pg_catalog', 'information_schema')
  AND NOT EXISTS (
    SELECT 1
    FROM pg_index i
    WHERE i.indrelid = c.oid
      AND i.indkey[0] = a.attnum
  )
ORDER BY n.nspname, c.relname, a.attname
LIMIT {capped}
"#
    )
    .trim()
    .to_owned()
}

/// Unused indexes: `idx_scan = 0`, excluding PK / UNIQUE / constraint-backed
/// (matches Pro `DashboardData` unusedIndexes query).
pub fn find_unused_indexes(limit: u32) -> String {
    let capped = clamp_report_limit(limit);
    format!(
        r#"
SELECT s.schemaname || '.' || s.indexrelname AS index_name,
       s.schemaname || '.' || s.relname AS table_name,
       pg_size_pretty(pg_relation_size(s.indexrelid)) AS index_size,
       pg_relation_size(s.indexrelid) AS raw_size,
       pg_get_indexdef(s.indexrelid) AS index_definition
FROM pg_stat_user_indexes s
JOIN pg_index i
  ON i.indexrelid = s.indexrelid
LEFT JOIN pg_constraint c
  ON c.conindid = s.indexrelid
WHERE s.idx_scan = 0
  AND s.schemaname NOT IN ('pg_catalog', 'information_schema')
  AND c.oid IS NULL
  AND NOT i.indisprimary
  AND NOT i.indisunique
ORDER BY raw_size DESC
LIMIT {capped}
"#
    )
    .trim()
    .to_owned()
}

/// Approximate table bloat via dead-tuple ratio (not physical page bloat).
/// Documented simplified estimate — avoids heavy pgstattuple / check_postgres SQL.
pub fn bloat_report(limit: u32) -> String {
    let capped = clamp_report_limit(limit);
    format!(
        r#"
SELECT schemaname || '.' || relname AS table_name,
       n_live_tup AS live_tuples,
       n_dead_tup AS dead_tuples,
       CASE WHEN n_live_tup + n_dead_tup > 0
            THEN ROUND(100.0 * n_dead_tup / (n_live_tup + n_dead_tup), 1)
            ELSE 0 END AS bloat_pct,
       pg_size_pretty(pg_relation_size(relid)) AS table_size,
       last_autovacuum,
       last_vacuum
FROM pg_stat_user_tables
WHERE schemaname NOT IN ('pg_catalog', 'information_schema')
  AND n_dead_tup > 1000
ORDER BY bloat_pct DESC, n_dead_tup DESC
LIMIT {capped}
"#
    )
    .trim()
    .to_owned()
}

/// Catalog fallback: `*_id` columns with no FK that name-match another table's PK.
pub fn find_missing_fks_catalog(limit: u32) -> String {
    let capped = clamp_report_limit(limit);
    format!(
        r#"
WITH pk_tables AS (
  SELECT
    n.nspname AS schema_name,
    c.relname AS table_name,
    a.attname AS pk_column,
    lower(c.relname) AS table_lower
  FROM pg_constraint con
  JOIN pg_class c ON c.oid = con.conrelid
  JOIN pg_namespace n ON n.oid = c.relnamespace
  JOIN LATERAL unnest(con.conkey) WITH ORDINALITY AS ck(attnum, ord) ON true
  JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ck.attnum
  WHERE con.contype = 'p'
    AND n.nspname NOT IN ('pg_catalog', 'information_schema')
    AND array_length(con.conkey, 1) = 1
),
candidates AS (
  SELECT
    n.nspname AS schema_name,
    c.relname AS table_name,
    a.attname AS column_name,
    left(a.attname, length(a.attname) - 3) AS name_prefix
  FROM pg_attribute a
  JOIN pg_class c ON c.oid = a.attrelid AND c.relkind = 'r'
  JOIN pg_namespace n ON n.oid = c.relnamespace
  WHERE a.attnum > 0
    AND NOT a.attisdropped
    AND n.nspname NOT IN ('pg_catalog', 'information_schema')
    AND a.attname ~* '_id$'
    AND a.attname <> 'id'
    AND NOT EXISTS (
      SELECT 1
      FROM pg_constraint con
      JOIN LATERAL unnest(con.conkey) AS ck(attnum) ON true
      WHERE con.conrelid = c.oid
        AND con.contype = 'f'
        AND ck.attnum = a.attnum
    )
)
SELECT
  cand.schema_name || '.' || cand.table_name AS from_table,
  cand.column_name,
  pk.schema_name || '.' || pk.table_name AS suggested_ref_table,
  pk.pk_column AS suggested_ref_column,
  'naming_convention' AS detection
FROM candidates cand
JOIN pk_tables pk
  ON pk.schema_name = cand.schema_name
 AND (
      pk.table_lower = lower(cand.name_prefix)
   OR pk.table_lower = lower(cand.name_prefix) || 's'
   OR pk.table_lower = lower(cand.name_prefix) || 'es'
   OR (right(lower(cand.name_prefix), 1) = 'y'
       AND pk.table_lower = left(lower(cand.name_prefix), -1) || 'ies')
 )
WHERE cand.schema_name || '.' || cand.table_name
   <> pk.schema_name || '.' || pk.table_name
ORDER BY from_table, column_name
LIMIT {capped}
"#
    )
    .trim()
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ref_accepts_schema_dot_name() {
        assert_eq!(
            parse_ref("public.users").unwrap(),
            ("public".into(), "users".into())
        );
    }

    #[test]
    fn parse_ref_defaults_schema() {
        assert_eq!(
            parse_ref("orders").unwrap(),
            ("public".into(), "orders".into())
        );
    }

    #[test]
    fn parse_ref_rejects_injection() {
        let err = parse_ref("public.users; DROP").unwrap_err();
        assert!(err.contains("Invalid object reference"), "{err}");
    }

    #[test]
    fn parse_ref_rejects_empty() {
        assert!(parse_ref("").is_err());
        assert!(parse_ref("   ").is_err());
    }
}
