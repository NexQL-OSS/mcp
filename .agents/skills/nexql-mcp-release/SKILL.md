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

Releases are loosely tracked — no enforced cadence. `$ARGUMENTS` may carry a target version (e.g. `0.2.1`); if not, ask the user for the target version rather than guessing it from the latest tag or CHANGELOG heading (those have drifted from `Cargo.toml` before).

### Step 1: Confirm working tree
`git status` clean, and check the current branch — this repo uses feature branches (`feat/…` → PR to `main`), not commits straight to `main`.

### Step 2: (Optional) Pre-release quality gates
Ask if the user wants these run before bumping:
```bash
LIBCLANG_PATH=/usr/lib cargo clippy --workspace --all-targets -- -D warnings
LIBCLANG_PATH=/usr/lib cargo test --workspace
```

### Step 3: Bump version
Use the existing atomic bump script rather than hand-editing manifests — it covers `Cargo.toml` (workspace version + internal path-dep pins), `npm/package.json`, all `npm/packages/mcp-*/package.json` platform packages, and `mcpb/manifest.json`:
```bash
./scripts/bump-version.sh X.Y.Z
./scripts/bump-version.sh --check X.Y.Z   # verify no manifest was missed
```
The script does **not** touch `README.md`'s Docker tag examples (`ghcr.io/nexql-oss/mcp:X.Y.Z`, `nexql-mcp:X.Y.Z`) — update those by hand.

### Step 4: Sync `Cargo.lock`
```bash
LIBCLANG_PATH=/usr/lib cargo check --workspace
```

### Step 5: (Optional) CHANGELOG entry
If the user wants one, prepend a new `## [X.Y.Z] - YYYY-MM-DD` section to `CHANGELOG.md` following the existing Keep a Changelog format, summarizing the change.

### Step 6: Report
Show the diff (`git diff --stat`) and the resulting version. **Do not commit, tag, or push** unless the user explicitly asks — bump/build and release are separate approvals.

---

## 4. Troubleshooting & Verification Checklist

| Problem | Cause / Solution |
|---------|------------------|
| `no method named resolve_target` | Ensure method is inside `impl ToolRouter` and helper functions are outside. |
| `router.specs().len()` assertion failure | Update expected tool count assertion in `exec.rs` (unit test) and `tests/phase2_catalog.rs` (integration test). |
| `bindgen` / `pg_query` compile error | Ensure `LIBCLANG_PATH=/usr/lib` is exported in command invocation or environment. |
