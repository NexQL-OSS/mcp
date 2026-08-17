---
name: nexql-mcp-release
description: >-
  Version bump, commit/tag/push, monitor Release CI to completion, and retry up to
  five times on failure. Also covers multi-profile DB config and LLM client onboarding in nexql-mcp.
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
Show the diff (`git diff --stat`) and the resulting version. **Do not commit, tag, or push** unless the user explicitly asks — bump/build and release are separate approvals. When the user asks to ship, continue with **§4**.

---

## 4. Ship: commit, tag, push, monitor CI (max 5 attempts)

Run this loop when the user asks to release, publish, tag, or finish a version bump. Track **`release_attempt`** from 1 through **5**; stop only when Release CI succeeds or attempt 5 fails.

**Repo:** `nexql-oss/mcp` (standalone git repo). **Trigger:** push tag `vX.Y.Z` → `.github/workflows/release.yml`.

### 4.1 Pre-ship checklist

- `git status` clean (or only intentional release fixes staged).
- On `main` (or merge PR first — release tags must land on the commit you intend to ship).
- `./scripts/bump-version.sh --check X.Y.Z` passes for target version.
- Tag name is `vX.Y.Z` matching workspace `Cargo.toml` version.

### 4.2 Commit and push (each attempt)

```bash
cd mcp   # repo root
git add -A
git commit -m "$(cat <<'EOF'
<concise why-focused message — ci fix, manifest sync, etc.>
EOF
)"
git push origin main
```

Only commit when there are changes. Skip empty commits.

### 4.3 Tag and push (each attempt)

GitHub Actions re-runs use the **tag’s commit SHA**. CI/workflow fixes require **retagging** onto the new `main` tip — re-running a failed job alone keeps the old broken workflow/script.

```bash
VERSION=X.Y.Z
git tag -d "v${VERSION}" 2>/dev/null || true
git push origin ":refs/tags/v${VERSION}" 2>/dev/null || true
git tag -a "v${VERSION}" -m "v${VERSION}"
git push origin "v${VERSION}"
```

### 4.4 Monitor Release CI

```bash
gh run list --workflow=release.yml --limit 5
gh run watch <run-id> --exit-status
```

On failure:

```bash
gh run view <run-id> --log-failed
```

**Job order (partial parallel):**

| Job | Purpose |
|-----|---------|
| `verify-release-tag` | `./scripts/bump-version.sh --check` vs tag |
| `build-*` | Cross-platform binaries + npm platform artifacts |
| `docker` | GHCR image |
| `publish-cargo` | `./scripts/publish-crates.sh` (ordered crates.io publish + poll) |
| `publish-npm` | Platform packages + root `npm/` |
| `package-mcpb`, `sbom`, `render-homebrew-formula` | Release assets |
| `release` | GitHub Release upload |

**Success criteria:** workflow run **completed / success** — all required jobs green, GitHub Release created, crates on crates.io, npm packages published.

Poll every 1–2 minutes while `in_progress`. Full matrix often takes **10–20 minutes**.

### 4.5 On failure → fix → next attempt

1. Read `--log-failed`; identify root cause (build, publish script, missing CI dep, manifest drift).
2. Apply minimal fix in repo.
3. Increment `release_attempt`.
4. If `release_attempt > 5`: report failure, list what was tried, ask user how to proceed. **Do not retag again.**
5. Otherwise repeat **§4.2 → §4.3 → §4.4**.

Common fixes (see also §5):

- **Tag/manifest mismatch** — re-run `./scripts/bump-version.sh X.Y.Z`, commit, retag.
- **Linux keyring / dbus link errors** — ensure `libdbus-1-dev` in `release.yml`, `ci.yml`, `build-setup.yml`, `Dockerfile`.
- **`cargo publish` / crates.io** — use `./scripts/publish-crates.sh` only; **never** pass `--no-wait` (unsupported on stable cargo in CI). Script skips already-published crates and polls index between dependents.
- **crates.io index lag** — script treats upload timeout / “already exists” as success and polls `https://crates.io/api/v1/crates/<crate>/<ver>`.

**Local unblock (optional, user-approved):** with `CARGO_REGISTRY_TOKEN` set, `./scripts/publish-crates.sh` from repo root can finish cargo publishes without waiting for full Release matrix — still retag afterward so npm/GitHub release jobs run.

### 4.6 Done

Report: tag SHA, workflow run URL, crates.io versions confirmed, npm/GitHub Release links. Update `CHANGELOG.md` date if still placeholder.

---

## 5. Troubleshooting & Verification Checklist

| Problem | Cause / Solution |
|---------|------------------|
| `no method named resolve_target` | Ensure method is inside `impl ToolRouter` and helper functions are outside. |
| `router.specs().len()` assertion failure | Update expected tool count assertion in `exec.rs` (unit test) and `tests/phase2_catalog.rs` (integration test). |
| `bindgen` / `pg_query` compile error | Ensure `LIBCLANG_PATH=/usr/lib` is exported in command invocation or environment. |
| `unexpected argument '--no-wait'` | Remove `--no-wait` from publish path; use `scripts/publish-crates.sh` as-is. |
| `publish-cargo` failed mid-chain | Re-run full release via retag; script skips crates already on crates.io. |
| Re-run failed job still fails | Tag points at old commit — fix on `main`, retag, push tag (§4.3). |
| Release attempt budget exhausted | Stop after 5 commit/tag/monitor cycles; summarize blockers for user. |
