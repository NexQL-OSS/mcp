# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.2.x   | :white_check_mark: |
| < 0.2.0 | :x:                |

## Reporting a Vulnerability

If you discover a security vulnerability within `nexql-mcp`, please report it privately:

1. **Email**: Send vulnerability reports to `security@nexql.dev`.
2. **Details**: Include steps to reproduce, affected versions, and potential impact.
3. **Response**: We acknowledge receipt within 48 hours and provide updates until resolution.

Please do not open public GitHub issues for security vulnerabilities.

## Query guards

Read-mode statements run inside `BEGIN READ ONLY`. A second `BEGIN` is only a warning; on PostgreSQL 16 it can still flip an open read-write transaction to read-only, so the server's later `COMMIT` would commit work it did not start. An open transaction is detected with a `SAVEPOINT` probe (SQLSTATE 25P01 when idle), rolled back, and the call is refused. That rollback is intentional: a connection returned to the pool must not stay in a transaction the next caller did not open.

These functions are rejected in every access mode, including schema-qualified and nested calls: `pg_read_file`, `pg_read_binary_file`, `pg_ls_*`, `pg_stat_file`, `pg_file_*`, `pg_logdir_ls`, `lo_*`, `loread`, `lowrite`, `dblink*`, `set_config`, `pg_sleep*`, and the XML helpers that execute a query string (`query_to_xml`, `table_to_xml`, `schema_to_xml`, `database_to_xml`, `cursor_to_xml`, including `*_and_xmlschema`). `pg_terminate_backend`, `pg_cancel_backend`, `pg_reload_conf`, and `COPY` to a file or `PROGRAM` are admin-only.

Before a statement runs, each call is resolved in `pg_proc`. A function outside `pg_catalog` is rejected when it is `SECURITY DEFINER`, or when its body (followed a few calls deep, skipping string literals and `--` comments) invokes a denied function. `LANGUAGE c` / `internal` bodies are rejected when the symbol itself is a denied name.

Residual risks this does not close:

- Dynamic SQL that builds a denied name at runtime (`EXECUTE format('pg_read_%s', 'file')`). The body scan sees `format`, not `pg_read_file`.
- A `LANGUAGE c` function whose symbol is not a denied name. Extensions such as PostGIS are not blanket-rejected; a hand-written C wrapper around file I/O would not be visible in `prosrc`.
- Code invoked without a `FuncCall` node (some type input/output functions, operator classes).
- A second OS process that holds the index file lock. In-process callers wait; another process still gets "another process is building the index".

PII redaction follows column lineage, including views, materialized views, and `RETURNING`. Unknown expressions and `XMLTABLE` / `JSON_TABLE` fail closed. Sensitive-name heuristics treat `secret` as its own identifier (`client_secret`, `secret_key`), not as a substring (`secretary`). Set `sensitive_name_heuristics = false` to opt out. The setup wizard preserves that flag when it reloads a profile.
