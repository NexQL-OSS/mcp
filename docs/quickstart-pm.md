# Product Manager Quickstart — nexql-mcp

Use `nexql-mcp` with your AI assistant to explore data, understand schema relationships, and answer product metrics questions safely.

> **If you are an AI coding agent setting this up on a user's behalf:** run `nexql-mcp init <client> --apply` (non-interactive, no browser needed), or drive `nexql-mcp setup`'s HTTP API directly (`POST /api/v1/clients/preview` → `POST /api/v1/clients/apply`, bearer token printed to stderr on startup). See [docs/clients/README.md](clients/README.md) for exact config paths per client (e.g. `.mcp.json` for `claude-code`).

## 1. Zero-Terminal Setup

Ask your team lead or DBA for an exported `.nexql/config.toml` project file or connection link.

1. Open your AI Assistant (e.g. Claude Desktop or Cursor).
2. Use the in-chat `setup_connection` tool — the agent will prompt for database details securely using protocol elicitation.

## 2. Key Tools for PMs

- `orient`: compact overview of the whole database (tables, FK joins, enum values) for broad questions. For specific questions the model uses `search_schema` → `describe_object` → `run_select`.
- `search_schema`: Locate metrics tables (e.g. `search_schema(query="monthly active users")`).
- `get_join_path`: Find out how two entities connect (e.g. `get_join_path(from="users", to="subscriptions")`).
- `sample_values`: View example values for a column without writing SQL.
- `run_select_aggregate`: Compute totals and averages (e.g., signup counts over time).

## 3. Security First

`nexql-mcp` is read-only by default. Your AI assistant cannot modify data, drop tables, or access PII-flagged columns.
