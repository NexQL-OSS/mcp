# Reference — TypeScript sources to port

Paths are relative to the sibling `nexql-pro` checkout in the NexQL-OSS workspace.

## MCP protocol

| TS file | Rust target | Notes |
|---------|-------------|-------|
| `pro/src/mcp/NexqlMcpStdioHost.ts` / `pro/src/mcp/McpDefinitionProvider.ts` | `nexql-proto` | Stdio host wiring, binary resolution, and ephemeral profile launch; `MCP_SERVER_INSTRUCTIONS` stays verbatim |
| `pro/src/mcp/McpResourceProvider.ts` | `nexql-tools::resources` | `nexql://` URIs, cursor pagination |
| `pro/src/mcp/McpPrompts.ts` | `nexql-tools::prompts` | Four prompts, pure data |

## Tools

| TS file | Rust target | Notes |
|---------|-------------|-------|
| `pro/src/providers/chat/tools/ToolSpec.ts` | `nexql-tools::schema` | JSON Schema via `schemars` from typed arg structs |
| `pro/src/providers/chat/tools/ToolExecutor.ts` | `nexql-tools::exec` | SQL bodies copy verbatim; guards stronger via pg_query |
| `pro/src/commands/sql/profile.ts` | `nexql-tools::sql` | Monitoring SQL |
| `pro/src/commands/sql/monitoring.ts` | `nexql-tools::sql` | Perf tool queries |

## Schema index (dbindex)

| TS file | Rust target |
|---------|-------------|
| `pro/src/features/dbindex/types.ts` | `nexql-index::model` |
| `pro/src/features/dbindex/lexical.ts` | `nexql-index::lexical` |
| `pro/src/features/dbindex/joinPath.ts` | `nexql-index::joins` |
| `pro/src/features/dbindex/catalogQueries.ts` | `nexql-index::catalog` |
| `pro/src/features/dbindex/indexFormat.ts` | `nexql-index::migrate` |
| `pro/src/features/dbindex/IndexStore.ts` | `nexql-index::store` |
| `pro/src/features/dbindex/IndexQueryService.ts` | `nexql-index::query` |
| `pro/src/features/dbindex/IndexBuilder.ts` | `nexql-index::builder` |
| `pro/src/features/dbindex/embeddings.ts` | `nexql-index::embed` |
| `pro/src/features/dbindex/localEmbedder.ts` | `nexql-index::embed` |

## Dropped

- `select_connection_context` — replaced by MCP `elicitation/create`

## Golden-file test (phase 3 gate)

Run TS `IndexBuilder` and Rust builder against the same seeded schema; assert byte-identical manifests, shards, and postings.
