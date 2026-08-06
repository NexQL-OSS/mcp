# DBA Quickstart — nexql-mcp

Database inspection, performance tuning, DDL safety analysis, and lock monitoring with `nexql-mcp`.

## 1. Multi-Environment Profile Setup

Configure production and staging environments with explicit access policies:

```bash
# Add staging with write access
nexql-mcp profile add staging \
  --host staging-db.internal \
  --dbname app_db \
  --access-mode write

# Add production with read-only enforcement and PII masking
nexql-mcp profile add prod \
  --host prod-db.internal \
  --dbname app_db \
  --access-mode read \
  --deny-tables "audit_logs,credit_cards"
```

## 2. Key Tools for DBAs

- `auto_tune_query`: Analyzes execution plan metrics and recommends targeted index creations.
- `suggest_indexes`: Detects high sequential-scan tables and unindexed foreign keys.
- `check_ddl_safety`: Uses PostgreSQL AST parsing (`pg_query`) to inspect proposed DDL for locking risks (`AccessExclusiveLock`, missing `CONCURRENTLY`, table rewrites).
- `get_slow_queries`: Fetches slow queries from `pg_stat_statements`.
- `get_lock_info` & `get_active_queries`: Inspect active locks, blocking transactions, and running queries.
- `get_table_bloat` & `get_vacuum_stats`: Identify table and index bloat requiring maintenance.
