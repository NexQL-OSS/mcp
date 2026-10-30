---
layout: ../../layouts/DocsLayout.astro
slug: safety
eyebrow: Guide · 02
title: Access modes and guardrails
description: What your assistant can do in read, write and admin mode, and what NexQL refuses no matter which mode you pick.
---

Every connection has an **access mode**. It's the main switch for what the assistant may do, and it's checked on every call, by nexql-mcp and, in read mode, by Postgres too.

## The three modes

| Mode | What it allows | How to turn it on |
| --- | --- | --- |
| `read` (default) | Look at the schema and run `SELECT` / `WITH` queries. | Nothing to do. |
| `write` | Also `INSERT`, `UPDATE`, `DELETE` through [`execute_sql`](/docs/tools/execute-sql), [`edit_row`](/docs/tools/edit-row) and [`import_data`](/docs/tools/import-data). | `--access-mode write`, `access_mode = "write"` in a profile, or the "Allow changes to data" switch in Claude Desktop. |
| `admin` | Also schema changes and server operations: [`apply_ddl`](/docs/tools/apply-ddl), [`create_index_concurrently`](/docs/tools/create-index-concurrently), [`run_maintenance`](/docs/tools/run-maintenance), [`terminate_query`](/docs/tools/terminate-query). | `--access-mode admin` or `access_mode = "admin"`. |

Write and admin tools aren't even listed to the assistant until at least one connection allows them. After raising a profile's mode, restart your app so it sees the new tools.

Asking the assistant to raise a profile's mode through [`save_profile`](/docs/tools/save-profile) needs `confirm_elevated_access: true` on the call, so it can't happen as a side effect.

## Superusers are kept read-only

If the connection logs in as a database **superuser**, nexql-mcp warns you, and write or admin mode is quietly turned back to read-only unless you start it with `--i-know-what-im-doing`. The assistant is told why, so it can tell you.

A superuser can read files on the database server and run programs there. A prompt-injected query could misuse that. Connect as a role with only the rights it needs instead. This prints the SQL for a least-privilege read role:

```bash
npx -y nexql-mcp@latest profile readonly-role
```

```sql
CREATE ROLE nexql_readonly WITH LOGIN PASSWORD 'replace-with-a-secret';
GRANT pg_read_all_data TO nexql_readonly;
```

## Blocked in every mode

Some things are refused whatever the access mode, because they reach outside the database or hide what a query really does:

- Reading or listing server files: `pg_read_file`, `pg_read_binary_file`, `pg_ls_*`, `pg_stat_file`, large-object functions (`lo_*`).
- Reaching other servers: `dblink*`.
- Running SQL hidden in a string, which would skip every other check: `query_to_xml` and its siblings.
- Changing session settings (`set_config`) and stalling the server (`pg_sleep`).
- `CREATE DATABASE` and `DROP DATABASE`. NexQL works on objects *inside* a database, never on the list of databases.

Outside admin mode, `COPY` to or from a file or program and `pg_cancel_backend` / `pg_terminate_backend` are refused too.

## Checks on writes

- **One statement per call**, parsed by Postgres's own parser.
- **Transactions.** `execute_sql`, `edit_row`, `import_data` and `apply_ddl` run inside a transaction. An error rolls everything back.
- **Dry runs.** `dry_run: true` runs the change, shows before-and-after snapshots of the affected rows, then rolls back.
- **Destructive DDL asks you.** For a CRITICAL-risk statement such as `DROP TABLE` or `TRUNCATE`, `apply_ddl` asks you directly in your app when it supports MCP elicitation. Otherwise the call needs `confirm_destructive: true`. The assistant is told not to set that on its own.
- **Lock warnings.** [`check_ddl_safety`](/docs/tools/check-ddl-safety) flags changes that would lock or rewrite a table and suggests the safer form.

## Limits on every query

| Limit | Default | Change it with |
| --- | --- | --- |
| Rows returned | 50 per call, up to 500 | `max_rows` in a profile, or `--max-rows` (1 to 10,000) |
| Statement timeout | 30 seconds | `statement_timeout_ms` in a profile. A call can lower it. |
| Raised timeout for bulk writes | up to 10 minutes | `NEXQL_MCP_MAX_TIMEOUT_MS` |
| Result size | 20,000 characters | Results are cut, with a note saying so. |

## Narrowing what's visible

In a profile:

```toml
schemas = ["public", "billing"]   # only these schemas
deny_schemas = ["audit"]          # never these
deny_tables = ["auth.*", "public.api_keys"]
```

Denied tables don't appear in searches, and queries that touch them are refused. In Claude Desktop, the install form's schema list sets `schemas`. On the command line it's `--schemas public,billing`.

`--tools` (see [Tool sets](/docs/tool-sets)) controls how many tools the assistant *sees*. It's a context-size setting, not a security boundary: access mode is what decides what runs.
