# nexql-proto

Model Context Protocol (MCP) JSON-RPC types, stdio and HTTP handlers, elicitation, and roots capabilities for `nexql-mcp`.

## Features

- **JSON-RPC 2.0**: Hand-rolled, zero-dependency transport types supporting protocol versions up to `2024-11-05`.
- **Transports**: Stdio transport (framing with JSON lines) and HTTP transport (bearer token authentication).
- **Outbound Requests**: Protocol elicitation (`elicitation/create`) for secure credential prompting and roots listing (`roots/list`).
