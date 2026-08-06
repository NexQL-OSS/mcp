# Changelog

All notable changes to `nexql-mcp` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
