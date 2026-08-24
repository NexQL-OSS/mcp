# Making nexql-mcp agent-native, end-to-end

## Context

This plan comes directly out of a real session where I (Claude Code) used `nexql-mcp` v0.4.2 to set up and seed a 15M-row Postgres database — the exact workflow an "agent-native" tool needs to nail. It surfaced eight concrete friction points, from a config-corrupting bug to silent doc drift to a fully-built but undiscoverable non-interactive setup path. This plan is grounded in a full exploration of `/home/ric-v/Projects/nexql-oss/mcp` (three parallel research passes: policy/config code, tool-surface/setup-wizard code, and design synthesis) — every item below cites exact files/functions, not general advice.

**Opinion, up front:** the codebase is far more capable than its docs suggest — `orient`, `discover_tools`, per-connection statement timeouts, live profile hot-reload, and a fully scriptable `init <client>` + `setup` HTTP API already exist. The gap isn't missing engineering, it's that (a) a couple of real bugs undermine trust in the tools an agent is told to rely on (`save_profile`, `check_ddl_safety`), and (b) the agent-friendly paths that already exist are undocumented, so agents (including me, this session) end up hand-rolling worse workarounds instead. Fix the trust bugs and point agents at what's already built, and this becomes agent-native with surprisingly little new code.

**Goal of this plan:** a prioritized, concrete fix list an implementer (or I, in a future session) can execute directly — file by file — to close these gaps, respecting the repo's existing architecture (`policy + conn → index → tools → binary` layering, AST-based SQL validation, test-first discipline per `.claude/skills/nexql-mcp-dev/testing.md`).

**Decisions already made for this plan** (confirmed with the user):
- `--tools query/dba/meta/full` stays **advisory/context-budget-only** — document it as such; `nexql-policy`'s `access_mode` remains the real security boundary. No `call_inner` enforcement change.
- The `apply_ddl` **destructive-confirm gate is in scope** (Phase 1.3 below) — closing the false-"SAFE" / zero-friction-DROP gap is worth the added round-trip for HIGH/CRITICAL statements.

---

## Phase 0 — Same-day doc/error-string fixes (zero design risk)

All independent; can land as one PR.

1. **Stale "41 tools" text** — `crates/nexql-mcp/src/main.rs:95-98` (`--tools` help) and `crates/nexql-mcp/src/tui/onboarding.rs:92` both hardcode "41 tools"; real count is 54 (`nexql-tools/src/registry.rs:484`, `ToolName::ACTIVE.len()`). Root-cause fix: reference `ToolName::ACTIVE.len()` via `format!()` at both sites instead of a literal, so this class of drift (already fixed once before, per CHANGELOG v0.2.0, and already regressed) can't recur. Extend the existing drift-guard pattern (`nexql-tools/src/registry.rs:508`, `docs_tools_readme_matches_active_surface`) with two siblings: `cli_help_tool_count_matches_active_surface`, `onboarding_label_matches_active_surface`.

2. **`discover_tools` drops `"meta"` category silently** — schema (`nexql-tools/src/schema.rs:257-259`) documents `category: "query"|"dba"|"meta"|"write"`; the match in `exec.rs:1009-1016` only handles 3 of the 4 and falls through to unfiltered results for `"meta"`. Add the missing arm (filter to `META_PROFILE`'s set, `registry.rs:339-353`). Test: `discover_tools_category_meta_filters_to_meta_profile`.

3. **`META_PROFILE` doc comment says "5 core tools"**, actually 13 (`registry.rs:13` vs `:339-353`). Reword to avoid a literal: `"Minimal initial tool surface + discover_tools meta-tool for lazy tool activation."`

4. **`apply_ddl` doesn't disclose the CREATE/DROP DATABASE boundary** — `schema.rs:981`'s description lists `CREATE, ALTER, DROP, TRUNCATE` with no caveat that `CreatedbStmt`/`DropdbStmt` are unconditionally rejected in every access mode (`nexql-policy/src/sql.rs:304-328`, `is_forbidden_always`). Add a scope sentence to the schema description. Also fix the error itself: `write.rs:483-497`'s generic `"Security Error: DDL statement is not permitted."` gives an agent zero signal this is architectural, not a permission problem it could fix by requesting more access. Special-case in `apply_ddl`: after the existing `pg_query::parse` (already run by `assert_ddl_statement`), if the root node is `CreatedbStmt`/`DropdbStmt`, return: `"Security Error: CREATE DATABASE / DROP DATABASE are outside this server's scope — nexql-mcp manages objects within the connected database, not the database catalog. Connect directly to the target database, or use external tooling for database-level operations."` Test: `apply_ddl_create_database_gives_scope_error_not_generic_denial`.

5. **Document the CREATE/DROP DATABASE boundary as permanent, intentional scope** in `README.md`, `docs/REFERENCE.md`, and `docs/tools/README.md`'s `apply_ddl` entry — one short "scope boundary" callout, cross-referenced. Treat this as final (not a placeholder for a future `create_database` tool) — no other finding argues for lifting it; if a real need shows up later, it'd be a new, separately-scoped tool with its own confirm gate, not a change to `is_forbidden_always`.

6. **`save_profile` schema overpromises "update" semantics** (`schema.rs:309`) while the implementation destructively replaces (see Phase 1.1). Until 1.1 ships, correct the doc string to warn: *"replaces the named profile wholesale — omitted fields are cleared, not preserved."* Flip back to "merges" once 1.1 lands.

---

## Phase 1 — Correctness bug fixes

### 1.1 `save_profile` silently wipes previously-saved fields (real bug, not by design)

`exec.rs:1423-1500` (`save_profile_tool`) builds a fresh `ProfileConfig` from only the current call's args and calls `cfg.upsert_profile(name, p_config)` — `upsert_profile` (`nexql-conn/src/config.rs:191-197`) is a raw `HashMap::insert`, full replace. Any omitted field among `ProfileConfig`'s 17 `Option`/`Vec` fields (including `host`, `user`, `credential_provider`, `sslmode`, `deny_schemas`, `pii_columns`) reverts to empty. I hit this directly this session — a `save_profile` call meant to change only `dbname` wiped `host`/`user` and had to be manually repaired.

**Fix**, mirroring the merge idiom that already exists one file over (`nexql-conn/src/resolve.rs:76-110`, `ConnectionParams::merge_from`):
- `nexql-conn/src/config.rs`: add `impl ProfileConfig { pub fn merge_from(&mut self, other: &Self) }` — per-`Option` field: `if self.field.is_none() { self.field = other.field.clone(); }`. For the four `Vec<String>` fields (`schemas`, `deny_schemas`, `deny_tables`, `pii_columns`): if the current call's `Vec` is empty, inherit the existing profile's `Vec` wholesale (element-level union would make "remove a denied schema via re-save" impossible — document this choice in the method's doc comment).
- `nexql-tools/src/exec.rs` `save_profile_tool`: after building `p_config` (unchanged), look up `cfg.profiles.get(name)`; if present, `p_config.merge_from(existing)` before `upsert_profile`. Leave `upsert_profile` itself untouched — merging belongs at the call site, which knows "this is a resave."
- Add an explicit opt-out: new schema param `replace: boolean` (default `false`) — `"If true, replace the profile wholesale instead of merging with existing saved fields."`

**Tests** (alongside the 4 existing `save_profile` tests, `exec.rs` ~4700-4860, none of which currently exercise a second save against the same name):
- `save_profile_partial_resave_preserves_existing_fields`
- `save_profile_replace_true_wipes_omitted_fields`
- `save_profile_vec_fields_inherit_when_call_omits_them`
- Unit tests directly on `ProfileConfig::merge_from` in `nexql-conn` (empty-into-populated, populated-into-empty, both-populated-prefers-self).

### 1.2 `check_ddl_safety` gives a false "SAFE" verdict for CREATE/DROP DATABASE

`dba_guard.rs::analyze_ddl_safety` never imports `nexql-policy` and has a `_ => {}` catch-all that silently ignores `CreatedbStmt`/`DropdbStmt` — so `check_ddl_safety("CREATE DATABASE x")` returns `overall_risk: "SAFE", is_safe: true`, directly contradicting what `apply_ddl` does with the same string (see Phase 0.4). An agent instructed (correctly) to check safety before applying gets a false green light.

**Fix**: add explicit match arms for `CreatedbStmt`/`DropdbStmt` in `analyze_ddl_safety` that push a `DdlSafetyIssue` with wording matching Phase 0.4's error text, and set risk to `RiskLevel::Critical` (simplest — reuse the existing top tier rather than adding a new `Forbidden` variant; the issue *message* is what conveys "not performable at any risk tolerance," not a new enum value — keeps the JSON shape stable).

**Tests**: `check_ddl_safety_flags_create_database_as_forbidden_not_safe`, `..._drop_database_...`. Also add both statements as `expect = "reject"` cases to the existing security-corpus fixture, `nexql-policy/tests/fixtures/sql_corpus.toml` — this is the repo's established regression mechanism for exactly this class of case (per `testing.md`'s SQL injection corpus section) and should gain these two cases regardless of where else they're tested.

### 1.3 `apply_ddl` has no confirmation gate for destructive statements

`check_ddl_safety` already classifies `DropStmt`/`TruncateStmt`/`AlterTableCmd`-style risk (`dba_guard.rs`), but that classification isn't reused to gate `apply_ddl` itself — a single `apply_ddl` call with a `DROP TABLE`/`TRUNCATE` executes immediately in admin mode; `dry_run` is opt-in, not required.

**Fix**, mirroring `save_profile`'s existing `confirm_elevated_access` pattern (`exec.rs:1429-1449`):
- `write.rs::apply_ddl`: after `assert_ddl_statement`/`validate_write_sql` pass, call `dba_guard::analyze_ddl_safety(sql)` internally. If `overall_risk` is `Critical` and `dry_run` is false and a new `confirm_destructive` arg isn't `true`, reject with a message naming the specific classified risk (reuse the `DdlSafetyIssue` text) instead of executing.
- `schema.rs:979-990`: add `confirm_destructive: boolean` (default `false`) — `"Required (true) when the statement is classified CRITICAL risk by check_ddl_safety's rules (e.g. DROP TABLE, TRUNCATE). Call check_ddl_safety first, or dry_run:true to preview."`
- `dry_run: true` calls are inherently safe and must **not** require `confirm_destructive` — the gate is only on statements that would actually execute.

**Tests**: `apply_ddl_drop_table_without_confirm_destructive_is_rejected`, `apply_ddl_drop_table_with_confirm_destructive_succeeds`, `apply_ddl_dry_run_bypasses_confirm_requirement`, `apply_ddl_low_risk_ddl_unaffected` (e.g. `ALTER TABLE ... ADD COLUMN` must not newly require confirmation).

**Sequencing note**: land 1.3 as its own reviewed PR after 1.1/1.2 — it's the one item in Phases 0-1 that changes agent-facing call behavior (adds a round-trip), not just messaging or classification.

---

## Phase 2 — Per-call statement-timeout control for `execute_sql`/`apply_ddl`

`run_select` already exposes `timeout_ms` (`schema.rs:212-218`, consumed at `exec.rs:2357-2362` via `ToolSession::set_statement_timeout`, `session.rs:511-518`) — but only to *lower* it below `scope.caps.statement_timeout_ms`. `execute_sql`/`apply_ddl` (`write.rs`) have no `timeout_ms` param at all; their pooled connection's `statement_timeout` is fixed once at pool creation (`nexql-conn/src/pool.rs:18-26,58-61`, 30s default). This session's 15M-row seed required manually shrinking batch size (300K → 150K → 75K) by trial and error against that fixed ceiling — pure friction a tool parameter removes.

**Fix**:
- `schema.rs`: add `timeout_ms` to `ExecuteSql` (~895-916) and `ApplyDdl` (~979-990), same shape as `RunSelect`'s existing param, but explicitly documented as able to **raise**, not just lower, the timeout for that one call: `"Override statement_timeout for this call, in milliseconds. Unlike run_select, may exceed the profile's configured ceiling — useful for bulk INSERT/DDL. Bounded by NEXQL_MCP_MAX_TIMEOUT_MS."`
- `exec.rs`/`write.rs`: thread `timeout_ms: Option<u32>` through `execute_sql`/`apply_ddl`, calling the existing `session.set_statement_timeout(...)` before running — a direct copy of `run_select`'s existing call pattern, not new mechanism.
- **New safety bound**: add `NEXQL_MCP_MAX_TIMEOUT_MS` (generous default, e.g. 600000ms/10min) enforced in `set_statement_timeout` or the call site, so a single call can't hold a connection open indefinitely. This is the one new config knob in this plan — justified as a ceiling on the newly-added "raise" capability, not scope creep.
- **Do not** build a bulk-DML chunking helper — that's query-shape territory nexql-mcp doesn't own. Instead, close the documentation gap: add a short `docs/bulk-operations.md` (or a section in `docs/REFERENCE.md`) showing the recipe this session had to discover by hand — `timeout_ms` + agent-side chunked batches — replacing trial-and-error with one documented pattern.
- Low-priority/optional: surface the pool's configured `statement_timeout` more prominently in `db_health_check`'s output so an agent checking health before a bulk job sees the ceiling without guessing.

**Tests**: `execute_sql_timeout_ms_raises_above_profile_default`, `apply_ddl_timeout_ms_applied_before_execution` (mirror whichever pattern `run_select`'s existing `timeout_ms` test already uses), `execute_sql_timeout_ms_capped_by_server_max`.

---

## Phase 3 — Positioning the agent-setup path (highest leverage-to-effort item in this plan)

**This is the top-line fix.** `nexql-mcp init <client>` (`crates/nexql-mcp/src/main.rs` `Commands::Init`, `init_clients.rs`) is *already* fully non-interactive and scriptable — no browser, no TTY — and `nexql-mcp setup`'s HTTP API (`setup/wire.rs::preview_wire`/`apply_wire`, `client_targets.rs`'s 12 mergeable targets) *already* performs the actual config merge-and-write. This session I didn't know either existed and hand-wrote `claude mcp add` shell commands instead — worse, and more error-prone (I did in fact hit a real `save_profile` bug immediately after). The gap is 100% discoverability, not missing capability.

1. **Add `--apply` to `init <client>`** (`main.rs`, `Commands::Init` handling ~548-565): instead of only printing a snippet, reuse `client_targets::mergeable_targets()`/`config_path_for()` and the same merge machinery `setup/wire.rs::apply_wire` already uses, in-process — no HTTP round-trip. Turns "agent has to hand-parse and merge JSON" into one command: `nexql-mcp init claude-code --apply`. Mostly reuse of tested code (`setup/wire.rs` already has its own test module), not new logic. Test: `init_client_apply_writes_merged_config_for_claude_code`.

2. **Document the agent-setup path explicitly**, separate from the human "open your browser" flow — add a callout near the top of `README.md`'s Quickstart and each `docs/quickstart-*.md`: *"If you are an AI coding agent setting this up on a user's behalf: run `nexql-mcp init <client> --apply` (non-interactive, no browser needed), or curl `nexql-mcp setup`'s HTTP API directly (`POST /api/v1/clients/preview` → `POST /api/v1/clients/apply`, bearer token printed to stderr on startup)."* Name exact config paths per client (e.g. `.mcp.json` for `claude-code`, per `client_targets.rs:128`).

3. **Enrich `MCP_SERVER_INSTRUCTIONS`** (`nexql-proto/src/lib.rs:31-39`) — this is the one channel already *proven* to work (its existing schema-grounding paragraph is what correctly steered my behavior this session). Append, incrementally, one line per shipped fix above (don't front-load behavior that doesn't exist yet):
   - After 3.1: nothing needed here (setup happens before a session exists to read instructions from — this is a docs-only fix, not an instructions fix).
   - After 1.1: `"save_profile merges into the existing named profile by default — omitted fields are preserved. Pass replace: true to wholesale-replace instead."`
   - After 0.4/1.2: `"CREATE DATABASE / DROP DATABASE are always rejected in every access mode — this server manages objects within a connected database, not the database catalog. Do not retry with a higher access_mode."`
   - After Phase 2: `"For bulk INSERT/UPDATE/DDL that may exceed the default statement timeout, pass timeout_ms rather than retrying repeatedly."`

4. **Give `orient` "start here" framing** — it exists specifically as a one-call schema bootstrap (landed v0.3.0) but currently appears only alphabetically listed in `docs/tools/README.md` with no framing, and isn't mentioned in any `docs/quickstart-*.md`. Add a "Start here" subsection above the alphabetical tool list, and make `orient` the first example call in each quickstart doc.

5. **Document `--tools`/`NEXQL_MCP_TOOLS`** (`query`/`dba`/`meta`/`full`) in `README.md`/`docs/clients/README.md` as a context-window budgeting knob — reference `registry.rs`'s `*_PROFILE` consts rather than hardcoding counts (per Phase 0.1's root-cause fix). Per the resolved decision above, state plainly: *"This restricts what's advertised to the client, not what's callable — `nexql_policy`'s `access_mode` (read/write/admin) is the real security boundary. Pick a narrower profile purely to reduce what's loaded into your agent's context window."*

6. **Add a "Phase 10 — agent ergonomics" row to README's Roadmap table** (currently stops at Phase 9), retroactively covering `orient`/`discover_tools`/`--tools` (landed) plus this plan's items (in progress) — gives this category of work a tracked home instead of scattering across ad hoc fixes.

---

## Sequencing

1. **Phase 0 + Phase 3.1/3.2 (docs, `init --apply`)** — land together first. Independent, zero-to-low risk, highest visible impact (fixes the day-zero setup experience and all doc drift in one pass).
2. **Phase 1.1** (`save_profile` merge) — isolated, high-value correctness fix.
3. **Phase 1.2** (`check_ddl_safety` classification) — after Phase 0.4, so wording matches.
4. **Phase 1.3** (`apply_ddl` confirm gate) — after 1.2, as its own PR (behavior change).
5. **Phase 2** (`timeout_ms`) — independent, can run in parallel with Phase 1.
6. **Phase 3.3–3.6** (instructions, `orient` framing, `--tools` docs, roadmap row) — each trails its corresponding behavior change by one commit, per the note in 3.3.

## Verification

- `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` after each phase, per `CLAUDE.md`'s standard commands.
- Phase 1.2's two new cases (`CREATE DATABASE`, `DROP DATABASE`, both `expect = "reject"`) added to `nexql-policy/tests/fixtures/sql_corpus.toml`, run via the existing SQL-injection-corpus test harness (`testing.md`).
- Manual end-to-end check for Phase 3.1: run `nexql-mcp init claude-code --apply` against a scratch `.mcp.json`, confirm the merge matches what `nexql-mcp setup`'s browser flow would have produced (diff against `setup/wire.rs`'s existing test fixtures if any exist).
- Manual re-run of this session's actual workflow (create profile → `save_profile` a partial update → confirm fields preserved → `apply_ddl` a `CREATE DATABASE` and confirm the new scoped error → `apply_ddl` a `DROP TABLE` and confirm the confirm-gate fires → bulk-insert with `timeout_ms` instead of manual batch-shrinking) as a final smoke test that the original pain points are actually gone.
