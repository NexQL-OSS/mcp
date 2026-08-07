# Changelog

All notable changes to `nexql-mcp` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
