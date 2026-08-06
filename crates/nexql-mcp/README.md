# nexql-mcp

Binary entrypoint, CLI subcommand routing, TUI integration, client configuration generators, and server lifecycle for `nexql-mcp`.

## Command Line Interface

- `nexql-mcp serve` — Launch stdio/HTTP MCP server.
- `nexql-mcp doctor` — Run automated connection, index, and policy diagnostics.
- `nexql-mcp index build` — Build or refresh offline schema index for a connection.
- `nexql-mcp profile add / list / test` — Manage connection profiles.
- `nexql-mcp init <client>` — Auto-configure MCP client targets (Claude Code, Claude Desktop, Cursor, VS Code, Zed, etc.).
