# Local MCP testing

Phase 7 (extension cutover) waits until this path is solid.

## Fastest path (scripted)

```bash
cd mcp
cargo build -p nexql-mcp --release   # once
./scripts/local_mcp_smoke.sh
```

Starts a throwaway Postgres (`initdb`), seeds `users`/`orders`, runs `doctor` + `index build`, then drives stdio JSON-RPC (`initialize`, `tools/list`, `list_schemas`, `search_schema`, `prompts/list`, `resources/list`).

Reuse an existing database:

```bash
DATABASE_URL='postgres://user@localhost:5432/appdb' ./scripts/local_mcp_smoke.sh
```

## Interactive — MCP Inspector

```bash
cargo build -p nexql-mcp --release
# If smoke started temp PG, copy the URL it printed; or use your own:
npx -y @modelcontextprotocol/inspector \
  ./target/release/nexql-mcp \
  'postgres://user@localhost:5432/appdb'
```

## Wire into Cursor / Claude Desktop

```bash
./target/release/nexql-mcp init cursor 'postgres://user@localhost:5432/appdb'
./target/release/nexql-mcp init claude-desktop 'postgres://…'
```

Paste into the client config. Connection string is a **positional arg before** subcommands:

```text
nexql-mcp <url> doctor
nexql-mcp <url> index build
nexql-mcp <url>          # stdio server (default)
```

## Build notes (Arch / user-local clang)

See `CLAUDE.md` — `LIBCLANG_PATH`, `LD_LIBRARY_PATH`, `BINDGEN_EXTRA_CLANG_ARGS` if `pg_query` bindgen fails.
