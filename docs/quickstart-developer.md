# Developer Quickstart — nexql-mcp

Get `nexql-mcp` up and running in your local development workflow.

## 1. Quick Installation

```bash
# Via npx (zero permanent install)
npx nexql-mcp init claude-code

# Or install binary directly
cargo install nexql-mcp
```

## 2. Connect Your Database

Set `DATABASE_URL` in your `.env` or environment:

```bash
export DATABASE_URL="postgres://postgres:password@localhost:5432/my_dev_db"
```

Then run `nexql-mcp doctor` to verify:

```bash
nexql-mcp doctor
```

## 3. Wire into your AI Agent

Run the `init` subcommand for your client:

```bash
nexql-mcp init claude-code   # For Claude Code
nexql-mcp init cursor        # For Cursor
nexql-mcp init claude        # For Claude Desktop
```

## 4. Key Tools for Developers

- `search_schema`: Find tables and columns relevant to your feature (`search_schema(query="users and orders")`).
- `describe_object`: View column types, nullability, constraints, and foreign keys for a table.
- `run_select`: Execute read-only SQL queries with automatic PII masking and result limit safety.
- `explain_query`: Inspect query plan and execution cost.
