---
name: nexql-mcp-release
description: >-
  Workflow for multi-profile DB configuration, adding new LLM models & AI clients,
  executing workspace quality checks, version bumping, and publishing git tag releases in nexql-mcp.
---

# nexql-mcp Release, Model Onboarding & Multi-Profile Skill

Guide for adding new LLM clients, multi-profile database support, quality verification, version bumping, and tag release workflows in the standalone `nexql-mcp` Postgres MCP server workspace.

---

## 1. Multi-Profile Database Support

`nexql-mcp` supports multi-profile database configurations across both the Profile Manager TUI (`nexql-mcp tui`) and the Model Onboarding Wizard (`nexql-mcp onboarding`).

### Key Files & Implementation
- **[app.rs](file:///home/ric-v/projects/nexql-oss/mcp/crates/nexql-mcp/src/tui/app.rs)**: Tracks `checked_profiles: HashSet<String>` in `ProfileList`. Pressing `Space` toggles single profile checkmarks; `A` toggles all profiles.
- **[onboarding.rs](file:///home/ric-v/projects/nexql-oss/mcp/crates/nexql-mcp/src/tui/onboarding.rs)**: Manages `selected_profiles: Vec<bool>` in Step 2 connection configuration. Pressing `Space` toggles profile checkmarks; `A` toggles all profiles.
- **[ui.rs](file:///home/ric-v/projects/nexql-oss/mcp/crates/nexql-mcp/src/tui/ui.rs)**: Renders `[x]` / `[ ]` checkmarks for database profiles.
- **Server Arg Generation**: Automatically appends `--profile <name>` for every selected profile:
  ```json
  "args": ["--profile", "dev", "--profile", "staging"]
  ```

---

## 2. Adding Support for New LLMs & AI Clients

When onboarding new LLM tools or AI clients (e.g. `antigravity`, `deepseek`, `kimi`, `ollama`, `qwen`):

### Steps & File Locations
1. **[init_clients.rs](file:///home/ric-v/projects/nexql-oss/mcp/crates/nexql-mcp/src/init_clients.rs)**:
   - Add client key string to `SUPPORTED_CLIENTS`.
   - Add match arm in `init_snippet()` returning paste-ready MCP JSON/YAML snippet with target file comments.

2. **[client_targets.rs](file:///home/ric-v/projects/nexql-oss/mcp/crates/nexql-mcp/src/client_targets.rs)**:
   - Implement path resolution function (e.g. `antigravity_config_path()`, `deepseek_config_path()`).
   - Add `ClientTarget` to `mergeable_targets()`.
   - Update unit test `mergeable_targets_cover_twelve_clients` to assert the target key count.

3. **[docs/clients/README.md](file:///home/ric-v/projects/nexql-oss/mcp/docs/clients/README.md)**:
   - Document client key in `Supported clients` list and add copy-paste snippet examples.

---

## 3. Version Bump & Release Tag Workflow

Follow these exact steps when preparing a new tag release (e.g. `v0.1.6`):

### Step 1: Pre-Release Quality Gates
Ensure all workspace lints and tests pass cleanly:
```bash
LIBCLANG_PATH=/usr/lib cargo clippy --workspace --all-targets -- -D warnings
LIBCLANG_PATH=/usr/lib cargo test --workspace
```

### Step 2: Bump Version Across All Manifests
Update version string in all 10 project manifests:
- `Cargo.toml`: `[workspace.package] version = "X.Y.Z"`
- `mcpb/manifest.json`: `"version": "X.Y.Z"`
- `npm/package.json`: `"version": "X.Y.Z"` and all `optionalDependencies` version strings
- `npm/packages/mcp-win32-x64/package.json`: `"version": "X.Y.Z"`
- `npm/packages/mcp-darwin-arm64/package.json`: `"version": "X.Y.Z"`
- `npm/packages/mcp-darwin-x64/package.json`: `"version": "X.Y.Z"`
- `npm/packages/mcp-linux-arm64/package.json`: `"version": "X.Y.Z"`
- `npm/packages/mcp-linux-x64/package.json`: `"version": "X.Y.Z"`
- `README.md`: Docker tag examples `nexql-mcp:X.Y.Z`

### Step 3: Update `Cargo.lock`
Run `cargo check` to synchronize `Cargo.lock`:
```bash
LIBCLANG_PATH=/usr/lib cargo check --workspace
```

### Step 4: Git Commit & Tag Creation
```bash
git add -A
git commit -m "chore(release): bump version to vX.Y.Z"
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin main --tags
```

---

## 4. Troubleshooting & Verification Checklist

| Problem | Cause / Solution |
|---------|------------------|
| `no method named resolve_target` | Ensure method is inside `impl ToolRouter` and helper functions are outside. |
| `router.specs().len()` assertion failure | Update expected tool count assertion in `exec.rs` (unit test) and `tests/phase2_catalog.rs` (integration test). |
| `bindgen` / `pg_query` compile error | Ensure `LIBCLANG_PATH=/usr/lib` is exported in command invocation or environment. |
