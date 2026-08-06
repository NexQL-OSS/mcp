# Product Manager Quickstart — nexql-mcp

Use `nexql-mcp` with your AI assistant to explore data, understand schema relationships, and answer product metrics questions safely.

## 1. Zero-Terminal Setup

Ask your team lead or DBA for an exported `.nexql/config.toml` project file or connection link.

1. Open your AI Assistant (e.g. Claude Desktop or Cursor).
2. Use the in-chat `setup_connection` tool — the agent will prompt for database details securely using protocol elicitation.

## 2. Key Tools for PMs

- `search_schema`: Locate metrics tables (e.g. `search_schema(query="monthly active users")`).
- `get_join_path`: Find out how two entities connect (e.g. `get_join_path(a="users", b="subscriptions")`).
- `sample_values`: View example values for a column without writing SQL.
- `run_select_aggregate`: Compute totals and averages (e.g., signup counts over time).

## 3. Security First

`nexql-mcp` is read-only by default. Your AI assistant cannot modify data, drop tables, or access PII-flagged columns.
