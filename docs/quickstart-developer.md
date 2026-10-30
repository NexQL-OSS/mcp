# Developer Quickstart — nexql-mcp

Get `nexql-mcp` up and running in your local development workflow.

> **If you are an AI coding agent setting this up on a user's behalf:** run `nexql-mcp init <client> --apply` (non-interactive, no browser needed), or drive `nexql-mcp setup`'s HTTP API directly (`POST /api/v1/clients/preview` → `POST /api/v1/clients/apply`, bearer token printed to stderr on startup). See [docs/clients/README.md](clients/README.md) for exact config paths per client (e.g. `.mcp.json` for `claude-code`).

## 1. Quick Installation

```bash
# Recommended: npx, zero install, always the latest release
npx -y nexql-mcp@latest init claude-code

# Fallback: install a permanent binary (see README.md § Install for more options)
cargo install nexql-mcp
```

## 2. Connect Your Database

Set `DATABASE_URL` in your `.env` or environment:

```bash
export DATABASE_URL="postgres://postgres:password@localhost:5432/my_dev_db"
```

Then verify:

```bash
npx -y nexql-mcp@latest doctor
```

## 3. Wire into your AI Agent

Run the `init` subcommand for your client:

```bash
npx -y nexql-mcp@latest init claude-code   # For Claude Code
npx -y nexql-mcp@latest init cursor        # For Cursor
npx -y nexql-mcp@latest init claude        # For Claude Desktop
```

If you installed a permanent binary instead (cargo, GitHub Release, Docker, …), drop the `npx -y nexql-mcp@latest` prefix and call `nexql-mcp` directly for every command above.

## 4. Key Tools for Developers

- `orient`: compact overview of the whole database (tables, FK joins, enum values) for broad questions. For specific questions the model uses `search_schema` → `describe_object` → `run_select`.
- `search_schema`: Find tables and columns relevant to your feature (`search_schema(query="users and orders")`).
- `describe_object`: View column types, nullability, constraints, and foreign keys for a table.
- `run_select`: Execute read-only SQL queries with automatic PII masking and result limit safety.
- `explain_query`: Inspect query plan and execution cost.
