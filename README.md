# nexql-mcp

Standalone Postgres MCP server — schema-aware, read-only by default, installable everywhere.

NexQL Pro ships an in-process MCP server locked to VS Code (`pro/src/mcp/`). This repo extracts that capability into an independent Rust binary any MCP client can spawn: Claude Desktop, Cursor, VS Code Copilot, Zed, etc.

**Status:** Phases 0–6 + **Phase 7 extension cutover (stdio spawn)** + **Phase 8 HTTP (bearer)** + **Phase 9 write/admin tools** landed. Phase 4b breadth complete (41 tools). Full OAuth gateway stays pro-only; HTTP sessions/rate-limit polish TBD. See [docs/CUTOVER.md](docs/CUTOVER.md).

## Why this exists

Competing Postgres MCP servers expose `connect → run query → return rows`. Models hallucinate table names against schemas that do not exist. NexQL's moat is the offline schema index (TF-IDF, join graph with inferred FKs, value profiles, optional embeddings, RRF fusion) built in `pro/src/features/dbindex/`. This repo ports that index and the 22 read-only tools from Pro into a fast, trivially installable binary.

## Architecture

```
crates/
├── nexql-mcp/      CLI, subcommands, wiring (binary)
├── nexql-proto/    MCP JSON-RPC types, transports
├── nexql-tools/    tool registry, schemas, executors
├── nexql-index/    dbindex port (builder, store, lexical, joins, embed)
├── nexql-conn/     connection resolution, pool, credentials
└── nexql-policy/   access modes, allow/deny, PII, caps, audit
npm/                npx shim (per-platform optionalDependencies)
mcpb/               one-click Claude Desktop bundle
docs/               per-client setup, tool reference
```

Layering is one-directional: `policy` + `conn` are leaves → `index` → `tools` → binary. `nexql-tools` never depends on `nexql-proto`.

## Install

Pick whichever fits your workflow — all methods ship the same binary.

### npm / npx

```bash
npx -y nexql-mcp postgres://dev@localhost:5432/appdb   # one-off, no install
npm install -g nexql-mcp                                # or install it once
```

[`nexql-mcp`](https://www.npmjs.com/package/nexql-mcp) is a shim ([`npm/bin/nexql-mcp.js`](npm/bin/nexql-mcp.js)) that resolves the right prebuilt binary from a per-platform `optionalDependency` (`@nexql/mcp-<os>-<arch>`) — no Rust toolchain needed.

### cargo (crates.io)

```bash
cargo install nexql-mcp
```

Builds from source, so you need clang/libclang first (`pg_query`'s bindgen requires it):

```bash
sudo apt install clang libclang-dev   # Debian/Ubuntu
sudo pacman -S clang                  # Arch
```

### curl (prebuilt binary, no npm/cargo)

```bash
TAG=$(curl -fsSL https://api.github.com/repos/NexQL-OSS/mcp/releases/latest | grep -m1 '"tag_name"' | cut -d'"' -f4)
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)   TRIPLE=x86_64-unknown-linux-gnu ;;
  Linux-aarch64)  TRIPLE=aarch64-unknown-linux-gnu ;;
  Darwin-x86_64)  TRIPLE=x86_64-apple-darwin ;;
  Darwin-arm64)   TRIPLE=aarch64-apple-darwin ;;
  *) echo "no prebuilt binary for this platform — see the Releases page" >&2; exit 1 ;;
esac
curl -fsSL -o /tmp/nexql-mcp.tar.gz \
  "https://github.com/NexQL-OSS/mcp/releases/download/${TAG}/nexql-mcp-${TAG}-${TRIPLE}.tar.gz"
tar -xzf /tmp/nexql-mcp.tar.gz -C /tmp
sudo install -m 0755 "/tmp/nexql-mcp-${TAG}-${TRIPLE}/nexql-mcp" /usr/local/bin/nexql-mcp
nexql-mcp --version
```

Windows: grab `nexql-mcp-<tag>-x86_64-pc-windows-msvc.tar.gz` from the [Releases page](https://github.com/NexQL-OSS/mcp/releases/latest) and extract manually.

### Docker

```bash
docker build -t nexql-mcp:0.1.4 .
docker run --rm -i nexql-mcp:0.1.4 postgres://dev@host.docker.internal:5432/appdb
```

### From source

```bash
export LIBCLANG_PATH="${LIBCLANG_PATH:-/usr/lib}"   # or your llvm lib dir
cargo build --release -p nexql-mcp
./target/release/nexql-mcp postgres://dev@localhost:5432/appdb
```

## Set up a connection

**One-off**, no config — pass a connection string directly:

```bash
nexql-mcp postgres://dev@localhost:5432/appdb
```

**Saved profiles** — put connections in `~/.config/nexql-mcp/config.toml` (override the path with `NEXQL_MCP_CONFIG`):

```toml
default_profile = "local"

[profiles.local]
url = "postgres://dev@localhost:5432/appdb"
access_mode = "read"

[profiles.prod]
host = "prod.example.com"
dbname = "app"
user = "readonly_agent"
password_command = "op read op://vault/pg/password"   # never store plaintext secrets
sslmode = "verify-full"
access_mode = "read"
schemas = ["public", "billing"]
deny_tables = ["auth.*"]
pii_columns = ["public.users.ssn", "public.users.email"]
max_rows = 200
```

Full field reference: [docs/config.example.toml](docs/config.example.toml). Then run bare (`nexql-mcp`) to use `default_profile`, or `nexql-mcp --profile prod`.

**Test a connection** before wiring it into a client:

```bash
nexql-mcp postgres://dev@localhost:5432/appdb doctor
# or, for a saved profile (note: --profile goes before the subcommand):
nexql-mcp --profile prod doctor
```

**Guided setup** — an interactive profile editor plus one-keystroke wiring into whichever clients you use: `nexql-mcp tui` (see [Interactive TUI](#interactive-tui) below).

### Wire a client

```bash
nexql-mcp postgres://dev@localhost:5432/appdb init cursor
```

Supported `init` clients: `claude` | `claude-desktop` | `claude-code` | `cursor` | `vscode` | `vscode-copilot` | `zed` | `windsurf` | `continue` | `jetbrains` | `openai-agents`.

Per-client paste blocks: [docs/clients/README.md](docs/clients/README.md).

## Use with the NexQL VS Code extension

If you already use [`ric-v.postgres-explorer`](https://marketplace.visualstudio.com/items?itemName=ric-v.postgres-explorer) (+ NexQL Pro), you don't need any of the above — the extension can spawn this binary itself and reuse your existing saved connections instead of a separate `config.toml`.

1. Settings → search **NexQL: Mcp: Enabled** (`postgresExplorer.mcp.enabled`) → check it. Off by default.
2. That's it — it takes effect immediately (no reload needed) and picks up every connection already saved in `postgresExplorer.connections`. It shows up as an MCP server named **NexQL** in Copilot Chat / agent-mode tool pickers.

The extension resolves the binary in this order: `postgresExplorer.mcp.binaryPath` setting → `NEXQL_MCP_BIN` env var → a copy bundled with the extension → whatever `nexql-mcp` is on your `PATH` (i.e. anything installed via npm/cargo/curl above). Set `postgresExplorer.mcp.binaryPath` explicitly if you want the extension to use a specific install.

### Interactive TUI

```bash
nexql-mcp tui
```

Guided profile editor: add/edit/delete a connection profile, test-connect it live before saving, then pick any of 7 clients (Claude Desktop, Claude Code, Cursor, VS Code, Copilot Chat, Zed, Windsurf) to wire it into at once. Each selected client's real config file is read, merged (existing unrelated servers are preserved), shown as a diff, and only written after you confirm — a timestamped backup is kept alongside it. `continue` / `jetbrains` / `openai-agents` have no safe on-disk merge target, so those stay copy-paste snippets in the summary screen, same as `init`.

Keys: `n` new · `e`/Enter edit · `d` delete · `t` test · `w` wire into clients · `q` quit. Bare `nexql-mcp` (no URL, no flags) launches the TUI automatically when nothing else resolves a connection.

### Releases

Pushing a `v*` tag triggers [`.github/workflows/release.yml`](.github/workflows/release.yml): builds darwin arm64/x64, linux gnu arm64/x64, and windows x64, attaches archives to a GitHub release, publishes the npm packages, and publishes the workspace crates to crates.io in dependency order. Musl targets deferred until a clang-enabled musl builder is validated.

## Development

```bash
cargo check          # workspace compile
cargo run -p nexql-mcp -- doctor
cargo test -p nexql-mcp -- init_clients
cargo fmt --all
cargo clippy --workspace --all-targets
```

Read [CLAUDE.md](CLAUDE.md) and [docs/REFERENCE.md](docs/REFERENCE.md) before implementing.

## License

Apache-2.0 for all crates in this repo. Premium extensions (provider embeddings, team sync, hosted gateway) will live in a separate proprietary crate later.

## Roadmap

| Phase | Deliverable |
|-------|-------------|
| 0 | Spike: tokio-postgres + candle MiniLM proof |
| 1 | `nexql-conn` + `nexql-policy` + pg_query validator |
| 2 | MCP stdio transport + ~8 catalog tools |
| 3 | `nexql-index` (byte-compatible with TS format) |
| 4 | Full tool surface, resources, prompts, completions |
| 5 | Local embeddings + RRF fusion |
| 6 | v1.0 ship: cargo-dist, npm, brew, Docker, MCPB |
| 7 | Extension cutover — VS Code spawns binary via stdio MCP definition |
| 8 | Streamable HTTP + bearer token (`--http` / `NEXQL_MCP_HTTP_TOKEN`) — OAuth gateway = pro |
| 9 | Write/admin tools + `validate_write_sql` (opt-in `--access-mode write\|admin`) |

Full plan: internal design doc (federated-greeting-badger). Cutover details: [docs/CUTOVER.md](docs/CUTOVER.md).

## Reference implementation

TypeScript sources in the sibling `nexql-pro` checkout (chat still uses these; MCP HTTP stack removed):

- `pro/src/mcp/McpDefinitionProvider.ts` — stdio spawn of this binary
- `pro/src/mcp/NexqlMcpStdioHost.ts` — ephemeral profile + binary resolve
- `pro/src/providers/chat/tools/ToolSpec.ts`
- `pro/src/providers/chat/tools/ToolExecutor.ts`
- `pro/src/features/dbindex/*`
