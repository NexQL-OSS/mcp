# Changelog

All notable changes to `nexql-mcp` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security
- Read mode rejects file and server escape functions on the SQL AST, including schema-qualified and nested calls: `pg_read_file`, `pg_read_binary_file`, `pg_ls_*`, `pg_stat_file`, `pg_file_*`, `pg_logdir_ls`, `lo_*`, `loread`, `lowrite`, `dblink*`, `set_config`, `pg_sleep*`, and `query_to_xml` / `table_to_xml` / `schema_to_xml` / `database_to_xml` / `cursor_to_xml`. `pg_terminate_backend`, `pg_cancel_backend`, `pg_reload_conf`, and `COPY` to a file or program are admin-only. Read-mode SQL runs inside `BEGIN READ ONLY`, and a connection already in a read-write transaction is rolled back and refused.
- Calls to non-`pg_catalog` functions are rejected when the function is `SECURITY DEFINER` or its body (followed a few calls deep) invokes a blocked function. See `SECURITY.md` for the dynamic-SQL and C-language residuals.
- PII redaction follows column lineage through aliases, expressions, subqueries, CTEs, `RETURNING`, views, and materialized views. Unqualified names use the session `search_path`. Whole-row constructors over a sensitive relation are rejected. Sensitive-name heuristics (`password*`, `*_hash`, `*token*`, `secret` as its own identifier, `ssn`) apply even when `pii_columns` is empty; `secretary` is not treated as a secret. Set `sensitive_name_heuristics = false` to opt out. The setup wizard round-trips that flag.
- `nexql-mcp init <client> postgres://user:pass@…` no longer writes the password into client config, including unparseable URLs, percent-encoded secrets, and `?password=`. The URL is saved as a keyring (or encrypted-file) profile and the snippet references `--profile`. The first saved profile becomes the default. `init <client> --apply <url>` creates that profile and wires it. The saved TOML is scanned again after write and refused if a password is still present.
- `doctor`, `profile test`, the setup wizard, and the TUI warn when the connection is a superuser and print `nexql-mcp profile readonly-role` (SQL for a `nexql_readonly` role with `GRANT pg_read_all_data`).

### Fixed
- Tool calls that arrive while the startup index build holds the lock wait for that build instead of failing with "another process is building the index". `orient` reports a missing or failed index as an error (`indexStatus`) instead of `tables: []` with `isError: false`.
- MCP `readOnlyHint` / `destructiveHint` match behaviour: `save_profile`, `import_profile`, and `setup_connection` are not read-only; `discover_tools` and `check_ddl_safety` are. `switch_connection`, `resolve_target`, `rebuild_index`, and `refresh_index` are not read-only.
- `async-trait` 0.1.92 (lockfile): current stable Clippy rejects the bare `#[must_use]` that 0.1.91 emits on async trait methods (`clippy::double_must_use`).

## [0.5.0] - 2026-08-24

### Added
- **`nexql-mcp init <client> --apply`**: writes the merged MCP client config directly — no browser, no TTY, reuses the same merge machinery `nexql-mcp setup`'s HTTP API uses. The recommended path for an AI agent setting nexql-mcp up on a user's behalf.
- **Real human-in-the-loop confirmation for destructive DDL**: `apply_ddl` now asks the connected MCP client to elicit a direct yes/no from the human (`elicitation/create`) before running a CRITICAL-risk statement, when the client supports it — not just an agent-settable `confirm_destructive` flag. Falls back to the existing argument gate when elicitation isn't available or fails; never proceeds silently.
- **`apply_ddl` destructive-confirm gate**: CRITICAL-risk statements (e.g. `DROP TABLE`, `TRUNCATE`) are refused unless `confirm_destructive: true` is passed or `dry_run: true` previews instead. Closes a false-"SAFE" / zero-friction-DROP gap.
- **`timeout_ms` on `execute_sql`/`apply_ddl`**: may *raise* the statement timeout above the profile default for one call (unlike `run_select`'s, which can only lower it), bounded by new `NEXQL_MCP_MAX_TIMEOUT_MS` (default 10 min). `db_health_check` now surfaces both ceilings. See `docs/bulk-operations.md`.
- **`save_profile` `replace` argument**: opts back into the old wholesale-replace behavior when explicitly wanted.
- `orient` given "start here" framing across `docs/tools/README.md` and every quickstart doc.

### Fixed
- **`save_profile` silently wiped previously-saved fields** on a partial resave (e.g. changing only `dbname` used to clear `host`/`user`/etc.) — now merges into the existing profile by default.
- **`check_ddl_safety` reported a false "SAFE"** for `CREATE DATABASE` / `DROP DATABASE` — now classified CRITICAL, matching `apply_ddl`'s own rejection of those statements in every access mode.
- **`apply_ddl`'s generic denial** for `CREATE DATABASE` / `DROP DATABASE` replaced with a scope-boundary error explaining this server manages objects within a connected database, not the database catalog — signals "architectural, not a permission gap" instead of inviting a retry with higher `access_mode`.
- **`discover_tools(category: "meta")`** silently fell through to the unfiltered catalog instead of filtering to `META_PROFILE`.
- Stale tool-count literals ("41 tools") in `--tools` help and the onboarding TUI now compute from `ToolName::ACTIVE.len()` instead of a hardcoded number that had already drifted once before.

### Changed
- **Installation instructions now lead with `npx -y nexql-mcp@latest`** across `README.md`, `docs/clients/README.md`, and `docs/quickstart-developer.md` — zero install, always the latest release, works identically for a human or an AI agent. Every other method (quick-install script, cargo, uv, manual download, Docker, MCPB, Homebrew) is now framed as a fallback for when npx isn't an option.
- **Site (`nexql-mcp.astrx.dev`)**: accent palette re-sourced from nexql.astrx.dev's actual brand tokens (terracotta accent, blue, indigo, magenta) on this site's existing dark background; smallest text sizes and lowest-contrast muted grays bumped for readability; narrative player now pauses (instead of fighting) manual scrolling or a click on any message, with a 2-minute auto-resume; setup-flow prompts corrected to not assume a real database URL, to not stall agents without browser access, and to explicitly forbid installing any way other than the documented command.
- `--tools`/`NEXQL_MCP_TOOLS` documented as a context-window budgeting knob only — `access_mode` remains the real security boundary.

## [0.4.2] - 2026-08-19

### Changed
- **Setup wizard UI**: tighter spacing/type scale in `crates/nexql-mcp/assets/setup/index.html` (denser touch targets, smaller headings/labels, reduced panel padding).

### Added
- **`crates/nexql-mcp/build.rs`**: reruns the build on setup-wizard `index.html` changes.

## [0.4.1] - 2026-08-18

### Fixed
- **Clippy warnings under `-D warnings`**: `nexql-conn` credential resolution and `nexql-mcp` setup-wizard save/export handlers had accumulated `useless_format`, `collapsible_if`, `field_reassign_with_default`, and `too_many_arguments` lints; cleared for CI.

### Changed
- **Setup wizard UI**: refreshed `crates/nexql-mcp/assets/setup/index.html`.
- **Site**: `astrx.dev` story demo split into shared narrative/setup islands (`NarrativePlayer.tsx`, `SetupDemo.tsx`, `story-shared.ts`) plus a new `/setup` page.

## [0.4.0] - 2026-08-18

### Added
- **`nexql-mcp setup`**: a local, browser-based setup wizard (`crates/nexql-mcp/src/setup/`). Lists installed MCP clients, previews the exact config diff per file, and applies it — no manual JSON editing. Runs standalone via `npx -y nexql-mcp setup`, no local install required.

### Security
- **No automatic plaintext password files**: `profile set-password` and inline-password migration store credentials in the OS keyring first. NexQL never writes passwords to disk in plaintext. When the keyring is unavailable, it now falls back automatically to a per-profile, machine-local encrypted file (ChaCha20-Poly1305, key stored 0600 under `~/.config/nexql-mcp`, secrets dir 0700) instead of failing outright — a warning names the fallback and points at `password_command` / an explicit user-managed `password_file` as stronger alternatives.

### Changed
- **nexql-mcp.astrx.dev rewritten as a single page**: the multi-page docs/features/install site is replaced by an interactive replay of one production incident worked end-to-end through NexQL's tools, plus an install modal (npx per-client snippets, curl/PowerShell installer, direct-download fallback, and the new setup wizard).

## [0.3.3] - 2026-08-17

## [0.3.2] - 2026-08-17

### Fixed
- **OS keyring lookup by wrong name**: `resolve_profile` routed every credential resolution through a hardcoded probe name instead of the actual profile name, so `credential_provider = "keyring"` never matched the stored OS keyring entry. Fixed in the CLI (`profile test`/`profile use`), MCP tool session connect, and the TUI's test/copy-snippet flows.
- **Linux keyring persistence**: Release builds now enable the `sync-secret-service` keyring backend instead of the in-memory mock store, so passwords stored via `profile set-password` survive across separate CLI/MCP processes (requires a running Secret Service, e.g. gnome-keyring on WSL).
- **Keyring miss is fail-fast**: When a profile declares `credential_provider = "keyring"` but lookup fails, resolution now errors immediately with keyring guidance instead of connecting and failing with `password missing`.
- **Connection error detail**: `format_postgres_error` now appends the underlying error source when present.
- **TUI profile inspector**: Distinguishes OS-keyring-stored passwords from inline (plaintext-in-config) passwords.

## [0.3.1] - 2026-08-16

### Fixed
- **JSON Schema for array parameters**: Enhanced generation so array-typed tool parameters carry correct `items` schemas.
- **Lint**: Resolved clippy `-D warnings` in `nexql-tools`/`nexql-mcp`.

## [0.3.0] - 2026-08-15

### Added
- **OS keyring for secrets**: Connection passwords stored in the OS keyring instead of plaintext config, with migration of legacy plaintext credentials on load.
- **Statement timeout config**: Per-connection statement timeout setting.
- **`orient` tool**: One-call schema bootstrap for new sessions.
- **Post-execution critique heuristics**: `run_select`/`execute_sql` flag zero-row results and `LIMIT` without `ORDER BY`.
- **Columnar output**: Extended to every row-returning tool (previously `run_select` only).

### Fixed
- **`access_mode` elevation**: `save_profile` now requires explicit confirmation to elevate access mode.
- **Discoverability**: Keyword-rich tool metadata and `serverInfo` title.
- **`get_index_status`**: Reports missing index instead of erroring.
- **Join-path resolution**: Unqualified relation refs resolved before join-path BFS.
- **Tool schemas**: Per-parameter descriptions and `additionalProperties: false` added across tool specs.

### Changed
- **EXPLAIN-family tools consolidated**: 54 → 52 tools.

## [0.2.2] - 2026-08-08

### Fixed
- **Linux glibc portability**: Release binaries are now built on Ubuntu 22.04 (glibc 2.35) instead of 24.04 (glibc 2.39), so `npm install nexql-mcp` and GitHub release tarballs run on Ubuntu 22.04, Debian 12, RHEL 9, and similar distros. CI verifies the GLIBC symbol ceiling; `install.sh` preflights compatibility.

### Added
- **`scripts/verify-glibc.sh`**: CI gate that fails if a Linux binary requires glibc newer than 2.35.
- **Install/npm diagnostics**: `install.sh` and the npm shim detect glibc mismatches and suggest `cargo install`, Docker, or an OS upgrade.

## [0.2.1] - 2026-08-07

### Added
- **PII result redaction**: Query/read tool rows replace configured `pii_columns` values with `<redacted>` (policy-aware via `FROM`/`JOIN` table refs).
- **Index stale markers**: DDL write paths mark `(connection_id, database)` stale until `rebuild_index` / `refresh_index` clears it; agents can detect post-DDL index drift.
- **Live profile registration**: `ToolSession::register_profile` upserts connections after `save_profile` / import / setup without restarting the server.
- **MCP tool hints**: Specs advertise `readOnlyHint` / `destructiveHint` / `idempotentHint` / `openWorldHint` for client UX.
- **Typed cell → JSON**: Shared `cell_json` conversion for Postgres types (incl. arrays, money, timestamps) used by read and write tools.
- **Typed write parameters**: Insert/update bind JSON args to typed Postgres parameters instead of string-only binding.

### Fixed
- **Read table policy on SELECT**: `enforce_read_table_policy` applied on execute/read paths so `deny_schemas` / schema allowlists actually block reads.
- **Clippy `-D warnings`**: `field_reassign_with_default` in config tests, `needless_return` in write param binding, and `items_after_test_module` in `cell_json`.
- **Release packaging**: `package-mcpb.sh` writes an absolute output path; release workflow cleans cargo package staging dirs.
- **Version bump coverage**: `scripts/bump-version.sh` now syncs `server.json` and npm `optionalDependencies` pins (previously left at the prior release).

### Changed
- Nested `if let` chains refactored to pattern matching across conn/index/mcp/tools for clearer control flow.

## [0.2.0] - 2026-08-06

### Fixed
- **TUI stdio stream hijacking**: Non-TTY stdio invocations no longer launch terminal UI when connection resolution fails, returning structured MCP JSON-RPC errors instead.
- **Index key harmonization**: Fixed mismatch between builder (`index_ids`) and runtime `ToolSession` lookup paths (`dbindex/{host}_{db}` vs `dbindex/default/{db}`).
- **Profile policy wiring**: Configured profile policy fields (`schemas`, `deny_schemas`, `deny_tables`, `pii_columns`, `max_rows`, `access_mode`) are now properly wired into `ToolSession` policy filters.
- **Panic removal**: Replaced `.expect()` serialization panics in MCP resource/prompt handlers and cursor encoding with safe error responses.
- **Active connection indexing**: Server boot now builds schema index only for active connection rather than eagerly scanning all configured profiles.
- **Silent profile drops**: Unresolvable profile configurations now emit diagnostic warnings on stderr.

### Added
- **Native MCP remediation tools**: Added `rebuild_index`, `refresh_index`, and `run_doctor` tools.
- **Version bump automation**: Added `scripts/bump-version.sh` for atomic version synchronization across 14 repository manifests.
- **CI matrix expansion**: Multi-OS verification (Linux, macOS, Windows), MSRV checks, and Postgres integration test container.
- **Release profile optimization**: Added thin LTO, symbol stripping, and single codegen unit to release builds.

### Removed
- **`nexql-spike` crate**: Removed throwaway experiment crate and its heavy dependency overhead.

### Fixed (v0.2.0 readiness pass)
- **Tool count drift**: `README.md` and `docs/tools/README.md` undercounted the active tool surface (41/45) against the real 53 in `ToolName::ACTIVE`; both now list all 53, including the previously-undocumented `resolve_target`, `auto_tune_query`, `check_ddl_safety`, `discover_tools`, `rebuild_index`, `refresh_index`, `run_doctor`, `setup_connection`, `save_profile`, `test_profile`, `export_profile`, `import_profile`.
- **`cargo fmt` violation**: unwrapped `check_ddl_safety` test call in `exec.rs` was failing `cargo fmt --all -- --check` in CI.
- Added a regression test (`registry::tests::docs_tools_readme_matches_active_surface`) so the doc catalog can't silently drift from `ToolName::ACTIVE` again.

### Added (Phase 6 distribution close-out)
- **MCPB bundles**: `scripts/package-mcpb.sh` packages a per-platform `.mcpb` (Claude Desktop one-click install) from `mcpb/manifest.json` + the release binary; attached to every GitHub release.
- **Docker/GHCR publish**: `release.yml` now builds and pushes `ghcr.io/nexql-oss/mcp:<tag>` / `:latest` from the existing distroless `Dockerfile`.
- **SBOM**: CycloneDX JSON SBOM (`cargo-cyclonedx`) generated and attached to every release.
- **Homebrew formula**: `scripts/render-homebrew-formula.sh` renders `Formula/nexql-mcp.rb` (darwin/linux × arm64/x64) from the release's vendor tarball SHA256s and attaches it as a release asset — a tap repo is not yet published, so `brew install` isn't live end-to-end.
- **MCP Registry**: `server.json` (`io.github.nexql-oss/nexql-mcp`) plus `.github/workflows/publish-mcp-registry.yml`, which publishes via GitHub OIDC after `release.yml` succeeds on a tag.

### Added (Phase 8 close-out — HTTP sessions + rate limit)
- **`Mcp-Session-Id` lifecycle**: issued on `initialize`, required on every subsequent HTTP request (400 if missing, 404 if unknown/expired), released via `DELETE`.
- **Rate limiting**: fixed-window (60s) limit per bearer token, or per client IP when no token is configured; `--http-rate-limit` / `NEXQL_MCP_HTTP_RATE_LIMIT` (default 600, `0` disables).
- Known gap: the session store has no LRU eviction cap yet — acceptable for single-tenant loopback use, not yet hardened for long-running multi-client HTTP exposure.

### Added (Phase 5 close-out — embeddings exit gate)
- `crates/nexql-index/tests/embeddings_semantic_gate.rs` (`--features embeddings`): proves the real MiniLM embedder — not the `FakeEmbedder` used by the existing RRF-fusion unit tests — ranks a synonym query ("client") to `public.customers` when lexical search (zero postings for that term) finds nothing. This was the documented but previously untested Phase 5 exit gate.
- New `cargo test (embeddings)` CI step (`cargo test --workspace --features embeddings`). Skips gracefully rather than failing CI when the model can't be downloaded, matching the existing `embed.rs` test's behavior.
- Fixed 3 pre-existing warnings in `builder.rs` that only surfaced under `--features embeddings` (unused `AtomicBool`/`Ordering`/`LOCAL_MODEL_ID` imports, dead `EMBEDDINGS_FEATURE_WARNED` static) by gating them to the non-`embeddings` build where they're actually used.

## [0.1.6] - 2026-08-06

### Added
- **`dba_guard`**: DDL safety analysis tool.
- `Makefile` for workspace management.

## [0.1.5] - 2026-08-05

### Fixed
- **TLS**: accept unverified certs for `sslmode=require` (was rejecting valid connections).

## [0.1.4] - 2026-08-05

### Fixed
- **`nexql-conn`**: improved Postgres error detail formatting.

### Docs
- End-user install/connection/extension docs now that npm + cargo distribution are live.

## [0.1.3] - 2026-08-04

### Fixed
- **npm binary**: restored exec bit lost in artifact upload/download round-trip (binary was unrunnable after `npm install`).

## [0.1.2] - 2026-08-04

### Added
- Multi-profile connection resolve, full-schema index discovery.

### Fixed
- **cargo publish**: made idempotent — skips already-published crates instead of failing the pipeline.
- **crates.io publish-check**: send `User-Agent` header; its absence was read as a 403 (misinterpreted as "not yet published").

## [0.1.1] - 2026-07-30

### Added
- **`nexql-mcp tui`**: connection wizard + multi-client wiring.

### Fixed
- **Release CI**: cross-compile darwin-x64 from `macos-14` (Intel runner pool was starved).

## [0.1.0] - 2026-07-30

Initial release.

### Added
- Rust workspace scaffold for the standalone MCP server.
- Phases 0–2: MCP stdio server + ~8 catalog tools.
- Phase 3: full query surface, `nexql-index` CLI, golden gate test.
