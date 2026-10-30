# nexql-mcp

Standalone Postgres MCP server — schema-aware, read-only by default, installable everywhere.

NexQL Pro ships an in-process MCP server locked to VS Code (`pro/src/mcp/`). This repo extracts that capability into an independent Rust binary any MCP client can spawn: Claude Desktop, Cursor, VS Code Copilot, Zed, etc.

**Status:** Phases 0–6 + **Phase 7 extension cutover (stdio spawn)** + **Phase 8 HTTP (bearer, sessions, rate limit)** + **Phase 9 write/admin tools** landed. 54 tools across Schema, Query, Context, Perf, Write, and Admin. Full OAuth gateway stays pro-only; session-store LRU eviction cap not yet implemented. See [docs/CUTOVER.md](docs/CUTOVER.md).

## Why this exists

Competing Postgres MCP servers expose `connect → run query → return rows`. Models hallucinate table names against schemas that do not exist. NexQL's moat is the offline schema index (TF-IDF, join graph with inferred FKs, value profiles, optional embeddings, RRF fusion) built in `pro/src/features/dbindex/`. This repo ports that index plus 54 query/schema/DBA/meta tools from Pro into a fast, trivially installable binary.

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

**Scope boundary:** nexql-mcp manages objects *within* a connected database (tables, indexes, columns, …), not the database catalog itself. `CREATE DATABASE` / `DROP DATABASE` are rejected in every access mode — permanent, intentional scope, not a gap. See [docs/tools/README.md](docs/tools/README.md#scope-boundary-no-database-catalog-operations) and [docs/REFERENCE.md](docs/REFERENCE.md).

## Install

### npx — recommended, zero install

```bash
npx -y nexql-mcp@latest postgres://dev@localhost:5432/appdb doctor
```

`-y` skips the "ok to proceed?" confirmation prompt; `@latest` always resolves the newest published
release instead of whatever npx happens to have cached — always use both together. This is the desired
way to run nexql-mcp: no separate install step, nothing added to `PATH`, works identically whether a
human is typing it or an AI agent is running it on your behalf. Requires [Node.js](https://nodejs.org/)
(npx ships with it) — nothing else.

```bash
# Verify
npx -y nexql-mcp@latest --version

# Setup
npx -y nexql-mcp@latest setup  # initialize connections and db profiles

# Wire your MCP client (or run the guided wizard)
npx -y nexql-mcp@latest init cursor          # Cursor
npx -y nexql-mcp@latest init claude-desktop  # Claude Desktop
npx -y nexql-mcp@latest init vscode-copilot  # VS Code Copilot
npx -y nexql-mcp@latest tui                  # interactive profile + client wiring
```

Client config (`command`/`args`) uses the same pattern — no install step for the client to rely on either:

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "command": "npx",
      "args": ["-y", "nexql-mcp@latest", "--profile", "mydb"]
    }
  }
}
```

> **If you are an AI coding agent setting this up on a user's behalf:** run `npx -y nexql-mcp@latest init <client> --apply` — non-interactive, no browser needed, writes the merged client config directly (`.mcp.json` for `claude-code`, `claude_desktop_config.json` for `claude-desktop`, etc. — see [docs/clients/README.md](docs/clients/README.md) for exact paths per client). Or drive `nexql-mcp setup`'s HTTP API directly: `POST /api/v1/clients/preview` → `POST /api/v1/clients/apply` (bearer token printed to stderr on startup). Prefer this over hand-writing `claude mcp add`-style shell commands or hand-parsing/merging the client's JSON config yourself.

Per-client config paths and paste blocks: [docs/clients/README.md](docs/clients/README.md).

`npm install -g nexql-mcp` also works if you'd rather have a permanent `nexql-mcp` on `PATH` than repeat
`npx -y nexql-mcp@latest` — same shim, same [`@nexql/mcp-<os>-<arch>`](npm/bin/nexql-mcp.js) prebuilt
binaries, no Rust toolchain needed either way.

**Linux system requirements:** prebuilt GNU/Linux binaries target **glibc 2.35+** (Ubuntu 22.04, Debian 12, RHEL 9, and newer). If npx/npm fails with `GLIBC_2.39 not found`, use one of the fallback methods below (`cargo install` builds from source; Docker sidesteps glibc entirely). Musl/static Linux builds are not published yet.

### Other install methods (fallback)

Reach for one of these only if npx isn't an option — an offline/air-gapped environment, a CI image that
wants a pinned binary baked in, no Node.js available, or you simply prefer a permanent install. All ship
the same binary as the npm package.

<details>
<summary><strong>Quick install script (Linux / macOS / Windows)</strong> — installs a permanent binary, no Node.js required</summary>

**Linux & macOS** — downloads the latest release, installs to `/usr/local/bin` (or `~/.local/bin` if sudo is unavailable), then prints setup steps:

```bash
curl -fsSL https://raw.githubusercontent.com/NexQL-OSS/mcp/main/scripts/install.sh | bash
```

Pin a version:

```bash
NEXQL_MCP_VERSION=v0.2.2 curl -fsSL https://raw.githubusercontent.com/NexQL-OSS/mcp/main/scripts/install.sh | bash
```

**Windows** (PowerShell) — installs to `%LOCALAPPDATA%\Programs\nexql-mcp` and adds it to your user `PATH`:

```powershell
irm https://raw.githubusercontent.com/NexQL-OSS/mcp/main/scripts/install.ps1 | iex
```

Pin a version:

```powershell
$env:NEXQL_MCP_VERSION = "v0.2.2"; irm https://raw.githubusercontent.com/NexQL-OSS/mcp/main/scripts/install.ps1 | iex
```

Or download and run the scripts locally: [`scripts/install.sh`](scripts/install.sh) · [`scripts/install.ps1`](scripts/install.ps1). Once installed, drop the `npx -y nexql-mcp@latest` prefix and call `nexql-mcp` directly for every command above.

</details>

<details>
<summary><strong>uv (PyPI)</strong></summary>

[uv](https://docs.astral.sh/uv/) installs CLI tools from PyPI into isolated environments — same prebuilt binary, no Rust toolchain.

Install uv itself (if needed):

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh   # macOS / Linux
```

```powershell
irm https://astral.sh/uv/install.ps1 | iex        # Windows
```

Install nexql-mcp:

```bash
uv tool install nexql-mcp
uv tool update-shell    # once, if uv warns the tool bin dir is not on PATH
```

One-off without installing (uv's equivalent of `npx -y`):

```bash
uvx nexql-mcp postgres://dev@localhost:5432/appdb doctor
```

Pin a version:

```bash
uv tool install 'nexql-mcp==0.2.2'
```

Upgrade later:

```bash
uv tool upgrade nexql-mcp
```

> **PyPI status:** wheels are not published yet. Until the first PyPI release lands, use npx or the quick install script above. Maintainer steps: [docs/publish-pypi-uv.md](docs/publish-pypi-uv.md).

</details>

<details>
<summary><strong>cargo (crates.io)</strong> — builds from source</summary>

```bash
cargo install nexql-mcp
```

Needs clang/libclang first (`pg_query`'s bindgen requires it):

```bash
sudo apt install clang libclang-dev   # Debian/Ubuntu
sudo pacman -S clang                  # Arch
```

</details>

<details>
<summary><strong>Manual download</strong> — grab a release archive by hand</summary>

From the [Releases page](https://github.com/NexQL-OSS/mcp/releases/latest):

| Platform | Archive |
|----------|---------|
| Linux x64 | `nexql-mcp-<tag>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `nexql-mcp-<tag>-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `nexql-mcp-<tag>-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `nexql-mcp-<tag>-aarch64-apple-darwin.tar.gz` |
| Windows x64 | `nexql-mcp-<tag>-x86_64-pc-windows-msvc.tar.gz` |

Extract and put `nexql-mcp` (or `nexql-mcp.exe`) on your `PATH`.

</details>

<details>
<summary><strong>Docker</strong></summary>

Prebuilt, published on every release to [GHCR](https://github.com/NexQL-OSS/mcp/pkgs/container/mcp):

```bash
docker run --rm -i ghcr.io/nexql-oss/mcp:0.5.0 postgres://dev@host.docker.internal:5432/appdb
# or: ghcr.io/nexql-oss/mcp:latest
```

Or build locally from the distroless `Dockerfile`:

```bash
docker build -t nexql-mcp:0.5.0 .
docker run --rm -i nexql-mcp:0.5.0 postgres://dev@host.docker.internal:5432/appdb
```

</details>

<details>
<summary><strong>Claude Desktop (MCPB one-click bundle)</strong></summary>

Each release attaches a platform `.mcpb` bundle (`nexql-mcp-<vendor>.mcpb`) — download the one matching
your OS/arch from the [Releases page](https://github.com/NexQL-OSS/mcp/releases/latest) and double-click
to install into Claude Desktop. Built from [`mcpb/manifest.json`](mcpb/manifest.json) via
[`scripts/package-mcpb.sh`](scripts/package-mcpb.sh).

</details>

<details>
<summary><strong>Homebrew</strong></summary>

No published tap yet — each release renders a formula (`Formula/nexql-mcp.rb`, via
[`scripts/render-homebrew-formula.sh`](scripts/render-homebrew-formula.sh)) and attaches it as a release
asset for a future `homebrew-tap` repo to pick up. Until that tap exists, use npx or the quick install script above.

</details>

<details>
<summary><strong>MCP Registry</strong></summary>

Listed in the [official MCP Registry](https://registry.modelcontextprotocol.io) as
`io.github.NexQL-OSS/nexql-mcp` ([`server.json`](server.json)), published automatically after each
release via GitHub OIDC (no stored credentials) — see
[`.github/workflows/publish-mcp-registry.yml`](.github/workflows/publish-mcp-registry.yml).

- mcp-name: io.github.NexQL-OSS/nexql-mcp

</details>

<details>
<summary><strong>From source</strong></summary>

```bash
export LIBCLANG_PATH="${LIBCLANG_PATH:-/usr/lib}"   # or your llvm lib dir
cargo build --release -p nexql-mcp
./target/release/nexql-mcp postgres://dev@localhost:5432/appdb
```

</details>

## Set up a connection

> Commands below assume `nexql-mcp` resolves on `PATH`. If you're running via npx instead, prefix each one with `npx -y nexql-mcp@latest` in place of `nexql-mcp` — see [Install](#install).

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

**Guided setup** — an interactive profile editor plus one-keystroke wiring into whichever clients you use:

- `nexql-mcp tui` — terminal UI (see [Interactive TUI](#interactive-tui) below)
- `nexql-mcp setup` — browser UI on loopback (profiles, client wiring, npx/path launch toggle); also `npx -y nexql-mcp@latest setup`

### Wire a client

```bash
nexql-mcp postgres://dev@localhost:5432/appdb init cursor
```

Supported `init` clients: `claude` | `claude-desktop` | `claude-code` | `cursor` | `vscode` | `vscode-copilot` | `zed` | `windsurf` | `continue` | `jetbrains` | `openai-agents`.

Per-client paste blocks: [docs/clients/README.md](docs/clients/README.md).

### Tool surface / context budget

`--tools query|dba|meta|full` (env: `NEXQL_MCP_TOOLS`) restricts which tools are *advertised* to the client — a context-window budgeting knob, not a security boundary. It's purely advisory: `nexql-policy`'s `access_mode` (`read`/`write`/`admin`, set via `--access-mode` or a profile's `access_mode`) is what actually gates what a call can do. Pick a narrower profile only to reduce how much tool-schema JSON gets loaded into your agent's context window:

- `query` (default) — core query/discovery tools (schema search, `run_select`, `explain_query`, …)
- `dba` — monitoring/admin tools (health checks, index suggestions, locks, …)
- `meta` — the smallest initial surface, plus `discover_tools` for lazy on-demand activation of the rest (see [docs/tools/README.md](docs/tools/README.md#start-here))
- `full` — every active tool

Write and admin tools (`execute_sql`, `edit_row`, `apply_ddl`, …) are left out of `tools/list` unless at least one configured connection's `access_mode` can run them, whatever `--tools` says. Restart the client after raising a profile to `write` or `admin`.

Exact membership per profile: `nexql-tools/src/registry.rs`'s `QUERY_PROFILE`/`DBA_PROFILE`/`META_PROFILE`/`ACTIVE` constants.

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

Pushing a `v*` tag triggers [`.github/workflows/release.yml`](.github/workflows/release.yml): builds
darwin arm64/x64, linux gnu arm64/x64, and windows x64; attaches archives, per-platform `.mcpb` bundles,
a CycloneDX SBOM, and a rendered Homebrew formula to a GitHub release; publishes the npm packages and
GHCR image; and publishes the workspace crates to crates.io in dependency order. A follow-up workflow
([`publish-mcp-registry.yml`](.github/workflows/publish-mcp-registry.yml)) then lists the release on the
MCP Registry via GitHub OIDC. Linux GNU binaries are built on Ubuntu 22.04 (glibc 2.35). Musl targets remain deferred until a clang-enabled musl builder is validated.

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

GPL-3.0-only for all crates in this repo, from v0.2.0 onward. If you distribute this
program or a derivative — including bundled inside another application — you must
release your source under the GPL as well.

Releases up to and including **v0.1.6** were published under Apache-2.0. That grant
is irrevocable for those versions and is unaffected by this change.

Copyright is held solely by the NexQL-OSS Team, so commercial licenses that lift the
GPL obligation are available on request. Premium extensions (provider embeddings,
team sync, hosted gateway) live in a separate proprietary crate.

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
| 10 | Agent ergonomics — `orient` one-call bootstrap, `discover_tools` + `--tools` context-budget profiles, `init <client> --apply`, `save_profile` merge semantics, `apply_ddl` destructive-confirm gate, per-call `timeout_ms` ([docs/roadmap/claude-review-2026-08-23.md](docs/roadmap/claude-review-2026-08-23.md)) |

Full plan: internal design doc (federated-greeting-badger). Cutover details: [docs/CUTOVER.md](docs/CUTOVER.md).

## Reference implementation

TypeScript sources in the sibling `nexql-pro` checkout (chat still uses these; MCP HTTP stack removed):

- `pro/src/mcp/McpDefinitionProvider.ts` — stdio spawn of this binary
- `pro/src/mcp/NexqlMcpStdioHost.ts` — ephemeral profile + binary resolve
- `pro/src/providers/chat/tools/ToolSpec.ts`
- `pro/src/providers/chat/tools/ToolExecutor.ts`
- `pro/src/features/dbindex/*`
