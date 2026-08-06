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
