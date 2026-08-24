# Data Analyst Quickstart — nexql-mcp

Accelerate data exploration, reporting, and query building with `nexql-mcp`.

> **If you are an AI coding agent setting this up on a user's behalf:** run `nexql-mcp init <client> --apply` (non-interactive, no browser needed), or drive `nexql-mcp setup`'s HTTP API directly (`POST /api/v1/clients/preview` → `POST /api/v1/clients/apply`, bearer token printed to stderr on startup). See [docs/clients/README.md](clients/README.md) for exact config paths per client (e.g. `.mcp.json` for `claude-code`).

## 1. Connection & Profile Setup

Add your database credentials using the CLI or let your assistant configure it via chat:

```bash
nexql-mcp profile add analytics \
  --host db.analytics.company.internal \
  --dbname analytics \
  --user analyst_readonly
```

## 2. Key Tools for Analysts

- `orient`: Call this first on an unfamiliar database — one-call schema bootstrap (tables, FK joins, enum-like columns) instead of several manual lookups.
- `search_schema`: Fuzzy search tables, views, and column comments across large catalog schemas.
- `sample_values`: Inspect distinct column values and distributions.
- `run_select_group`: Build `GROUP BY` aggregations dynamically.
- `run_select_window`: Run window functions (`ROW_NUMBER`, `RANK`, `LAG/LEAD`).
- `copy_to_csv`: Export query results directly to CSV format for external analysis.
- `compare_connections`: Compare schema differences between staging and production databases.
