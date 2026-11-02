# nexql-mcp — public-readiness checklist (2026-10-28)

How this list was made: I installed v0.5.0 the way a new user would (`npx -y nexql-mcp@latest`, clean `$HOME`) and also built from source. I ran `doctor`, the `setup` browser wizard, `init claude-desktop --apply` and `--http`. Then I drove the stdio MCP protocol directly, acting as the model, against the local `postgres-test` container: `ecommerce_core` (1 GB; 1.5M order_items, 1M orders, 500k products), `ecommerce_analytics` and `ecommerce_auth`.

Target user: a non-developer who wants to add their database to **Claude** (Desktop, web or mobile) or **ChatGPT**, then ask questions in natural language.

---

## P0: blockers (fix before any public push)

### 1. There is no way to use it from ChatGPT or claude.ai web/mobile
- ChatGPT can't run local stdio servers at all. It needs a **remote HTTPS** MCP URL with **OAuth** or no auth; API-key and static bearer auth aren't supported. claude.ai custom connectors work the same way (URL plus OAuth, no custom header field). Only Claude Desktop can use the stdio binary.
- Today `--http` offers only a static bearer token:
  - `/.well-known/oauth-protected-resource` and `/.well-known/oauth-authorization-server` both return 404.
  - A 401 response has no `WWW-Authenticate` header, so clients can't discover how to authenticate.
  - It binds to loopback, and there's no tunnel or public-URL story.
- `init chatgpt` → `unknown client 'chatgpt'`. ChatGPT and claude.ai aren't mentioned in the README, `docs/clients`, the TUI or the wizard.
- The README says "Full OAuth gateway stays pro-only". **That's a product decision to make now:** if OAuth stays paid, free users can't use ChatGPT or Claude web at all.
- To do:
  - [ ] OAuth 2.1 resource server: PKCE, Protected Resource Metadata, a `WWW-Authenticate` header on 401, and client registration via CIMD (plus DCR for older clients).
  - [ ] A way to get a public HTTPS URL. Either `nexql-mcp serve --public`, which wraps cloudflared or ngrok and prints the URL to paste, or a hosted gateway that users reach their DB through. A user's DB is usually on localhost or a private VPC, so a hosted gateway also needs an agent or tunnel.
  - [ ] Per-session state over HTTP. Today one `ToolSession` backs every HTTP session, so `switch_connection` in one chat probably moves the others too. I didn't verify this.
  - [ ] Session-store LRU/TTL cap (listed as not implemented in the README).
  - [ ] Client docs: "Add to ChatGPT (Developer Mode → Connectors)" and "Add to Claude (Settings → Connectors → Add custom connector)".

### 2. Read-only mode on a superuser can read server files (exfiltration risk)
- `run_select` → `SELECT pg_read_file('/etc/passwd')` **returned the file**. `check_superuser_guard` (`crates/nexql-policy/src/access.rs:45`) only blocks superusers in write/admin mode.
- Most non-technical users will paste their `postgres` or admin URL. Combined with prompt injection from data in rows, this lets anyone who can get text into the DB read server files.
- To do:
  - [ ] Add a function denylist enforced on the pg_query AST: `pg_read_file`, `pg_read_binary_file`, `pg_ls_*`, `pg_stat_file`, `lo_*`, `dblink*`, `pg_terminate_backend`, `pg_cancel_backend`, `pg_reload_conf`, `set_config`, `pg_sleep`.
  - [ ] Treat superuser as a **red warning** in doctor, the wizard and the TUI. Today it shows as a neutral `superuser=true`. Better still, offer one click to create `nexql_readonly` (`GRANT pg_read_all_data`) and use that role instead.
- What holds up: DML, multi-statement queries and writable CTEs are rejected. `set_config('default_transaction_read_only','off')` runs, but `nextval()` is still blocked afterwards, so read-only transactions stay intact.

### 3. PII redaction is cosmetic
With `pii_columns = ["customers.users.email", "customers.users.phone"]`:
- `SELECT email, phone …` → `<redacted>` ✅
- `SELECT email AS contact`, `lower(email)` → **raw emails** ❌
- `SELECT row_to_json(u) FROM customers.users u` → raw email, phone **and `password_hash`** ❌

To do:
- [ ] Track column lineage in the AST. Reject or redact any expression that uses a PII column, plus whole-row functions (`row_to_json`, `to_jsonb(t)`, `t.*` inside a function).
- [ ] Add default heuristics (`password*`, `*_hash`, `*token*`, `*secret*`, `ssn`) that are masked even when the user configures nothing.

### 4. The Claude Desktop config it writes doesn't start for npx users
- `init claude-desktop --apply`, the wizard, `docs/clients/README.md` and every snippet write `"command": "nexql-mcp"`. The README's **recommended** install is `npx`, which puts nothing on `PATH`, so Claude Desktop fails with spawn ENOENT.
- GUI apps on macOS also don't inherit the shell `PATH` (nvm, Homebrew), so even `npx` often isn't found.
- To do:
  - [ ] Write the absolute path of the resolved binary, or the absolute path to `npx` plus `-y nexql-mcp@latest`.
  - [ ] Make the **.mcpb bundle the primary Claude Desktop path**. It's already built for 5 platforms, but users have to pick the right architecture file from GitHub Releases. Ship one landing page with an OS-detecting "Add to Claude Desktop" button.

### 5. Credentials and the `--apply` flow
- `init claude-desktop postgres://user:pass@…` puts the **plaintext password into the client JSON**.
- `init <client> --apply` with a URL fails with `no saved profile to wire`, even though the README tells AI agents to use exactly this. It should save a keyring profile automatically, then wire it.

---

## P1: first-run experience

6. **Race with the background index build.** If a tool call arrives while the startup auto-index build is still running, it fails with `Automatic index build failed: another process is building the index`. I reproduced this twice.
   - `orient` returns `tables: []` with `isError: false`, so the model believes the DB is empty.
   - Fix: await the in-flight build through a shared future instead of erroring (`crates/nexql-tools/src/exec.rs:1660`, `crates/nexql-mcp/src/main.rs:920`).
   - The build itself is fast (97 ms for 24 tables and 135 functions).
7. **Superuser** shows up as plain info everywhere (see #2).
8. **MCPB `user_config`** asks only for `DATABASE_URL`. Add an access mode (default read), allowed schemas, and a "Test connection" hint.
9. **Wizard polish.**
   - The review step shows `Auth: none` even though a password was supplied and moved to the keyring.
   - The review step shows the password in plaintext in "Target".
   - The wizard leaves a `__setup_availability_probe__` entry in the OS keyring.
10. **CLI help.**
    - Most flags have no help text (`--profile`, `--host`, `--stdio`, `--http`, `--max-rows`, …).
    - `setup --no-open` is documented as "Open the setup UI in the default browser" (inverted).
11. **Setup HTTP API.** The README tells agents to call `POST /api/v1/clients/preview`, but the body shape (`client_keys`, `copy_only_keys`, `profile_names`, `launch`) is undocumented. I hit 4 rounds of 422 errors before getting a request accepted. Publish the schema or drop it from the README.
12. **Logs.** They land in `~/.config/nexql-mcp/logs` (config dir rather than the state dir) and say only `error connecting to server`, with no host or reason.

## P1: how well natural-language querying works (as the model sees it)

13. **Too many tools by default.** `full` advertises 54 tools and about 38 KB of `tools/list` (~10k tokens) before the user types anything.
    - Write and admin tools (`execute_sql`, `apply_ddl`, `terminate_query`, …) are **advertised in read mode** and only fail when called.
    - For consumer use, the default should be `query`, and the server should hide tools the current access mode can't run.
14. **Wrong tool annotations.** ChatGPT asks for confirmation on any tool that isn't marked read-only, and directory reviewers check these flags.
    - `save_profile`, `import_profile` and `setup_connection` say `readOnlyHint: true` but write config.
    - `discover_tools` and `check_ddl_safety` say `false` but are read-only.
15. **Conflicting "call this first".** The server instructions say list_schemas or search_schema first. `orient` says "Call this FIRST". `search_schema` also says "Call this FIRST". Pick one flow: `orient` → `describe_object` → `run_select`.
16. **orient is too big.** It returns **34 KB of pretty-printed JSON for 24 tables**, which won't scale to 500-table schemas.
    - Make it compact and paginated.
    - `enums: {}` even though `status` columns are clearly enum-like, because the default `Structure` depth skips value profiles. Profile low-cardinality text columns by default.
17. **Inconsistent argument names.** `describe_object(ref)`, `sample_values(col)`, `get_join_path(a, b)`. As the model, I guessed wrong 3 out of 3 times. Standardize on `table` / `column` / `from` / `to`, and accept aliases.
18. **Cross-database calls insist on `connectionId`.** `list_databases`, `switch_connection` and `run_select(database=…)` all fail with "connectionId is required", even with exactly one active connection. They should default to the active one.
    - `search_all_databases` returned `searched_pairs: 0` without saying why. It should auto-index or explain.
19. **Output shape for chat apps.**
    - Every result is JSON inside text, prefixed with `[context …]`, and duplicated in `structuredContent`.
    - Chat users want tables: default to markdown or compact output for `run_select`.
    - `COUNT(*) OVER()` pagination makes Postgres compute the full result for every query. Use `LIMIT n+1` and make the total count opt-in.
20. **Search ranking.** `search_schema("monthly revenue from orders")` ranks `billing.transactions` above `orders.orders` and the `mv_daily_sales_summary` matview. Weight names and matviews higher, and add column-name matches.

Things that worked well and should be kept:
- Helpful `fix_hint` when a column doesn't exist.
- Query critique (seq scan on a large table, LIMIT without ORDER BY).
- 30 s statement timeout with an actionable error.
- Aggregating 1M rows by month: 2.2 s.
- Keyring storage for wizard-saved passwords.

## P2: launch hygiene

- [ ] **README diet.** It lists 9 install methods. Two are unavailable (PyPI wheels "not published yet", no Homebrew tap). For the public, show 2 paths (Claude Desktop .mcpb; ChatGPT/Claude web via remote URL) and move the rest to docs.
- [ ] **Directory listings** (Anthropic connector directory, ChatGPT apps). These need a hosted remote URL, a privacy policy, a support contact, accurate tool annotations (#14), a logo and a test account.
- [ ] **Data-flow consent.** In remote mode, query results go to OpenAI or Anthropic. Say so in setup.
- [ ] **Prompt-injection threat model** in SECURITY.md: DB rows are untrusted input to the model.
- [ ] **Per-user query audit log** that users can review.
- [ ] **`nexql-mcp demo`** with a bundled sample DB, so people can try it without connecting production.
- [ ] Optional crash/usage telemetry (opt-in), so you can see where setup fails.

## Suggested order

1. **Safety and correctness** (#2, #3, #5, #6, #14). Cheap, and required whatever the distribution channel.
2. **Claude Desktop that just works** (#4, #8, #13, #15–18). One-click .mcpb, read-only role helper, smaller tool surface.
3. **Remote + OAuth** (#1). This unlocks ChatGPT and claude.ai web/mobile, starting with local-server-plus-tunnel, then a hosted gateway.
4. **Directory submissions and docs** (P2).

---

## Dry run, 2026-10-29 (after batch 1, PR #18 at `dfe76fa`)

Setup: the PR-branch release binary as the only MCP server in headless Claude Code (Sonnet, built-in tools off). Profile as a new user would have it after `init`: URL plus password, `access_mode = "read"`, no `pii_columns`. Nine fresh sessions, one per question, against `ecommerce_core` (and `ecommerce_analytics` for one).

**Result: 9/9 answers correct.** Every number was checked against the database by hand. There were 47 tool calls in total (about 5 per question), 20 s and about $0.05 per question, and $0.44 overall. No tool failed apart from the expected write refusal. The cold-start index build was invisible to the user. The model grounded itself every time (`search_schema` → `describe_object` → `run_select`) and raised real data caveats: a partial month, an ambiguous "payment method type", a feed that stopped on 10-27, and products with only one review each, so a "≥5 reviews" ranking is impossible.

| # | Question | Calls | Outcome |
|---|---|---|---|
| 1 | Revenue by month this year | 4 | ✅ exact |
| 2 | Worst-rated products (≥5 reviews) | 5 | ✅ correctly says none qualify |
| 3 | Refund rate by payment method | 6 | ✅ exact; flagged provider vs gateway ambiguity |
| 4 | Customers with >3 orders in 90 days | 3 | ✅ exact (16,177) |
| 5 | Warehouse with most low stock | 5 | ✅ correctly says none |
| 6 | Web traffic vs orders, 2 databases | 10 | ✅ exact, but slow discovery |
| 7 | Names + emails of top 5 customers | 5 | ⚠️ emails returned in full |
| 8 | "Delete all cancelled orders" | 5 | ✅ refused; tried `execute_sql` first |
| 9 | "How is the business doing?" | 4 | ✅ sensible; labelled amounts `$` (data looks like INR) |

What the dry run changes in the plan:

- **PII is exposed by default (new top item).** A user who never configures `pii_columns` gets full emails, and phones would come back the same way. The name heuristics only cover secrets (`password`, `*_hash`, `token`, `secret`, `ssn`). Setup should auto-detect personal-data columns from the index (`email`, `phone`, `address`, names) and propose them, on by default.
- **Write tools in read mode invite attempts (#13).** In Q8 the model called `execute_sql` before giving up. Hide tools the access mode cannot run.
- **Most tools went unused (#13).** 11 of 54 tools were used across nine questions. Every chat still pays about 10k tokens for the 54 tool schemas, so a `query`-sized default is safe.
- **Cross-database discovery is the slowest path (#18).** Q6 took 10 calls: `search_all_databases` found nothing (the analytics DB has no index), so the model fell back to querying `information_schema`. Index other databases on demand.
- **The "call this first" guidance is ignored (#15).** `orient` was used in 2 of 9 sessions and cost 14 KB each time. The model prefers `search_schema` → `describe_object`, so make that the documented path and slim `orient` down.
- **Inconsistent argument names (#17) did not hurt Sonnet.** It read the schemas and used `ref` correctly every time. Lower priority, but recheck with a smaller model.

Not covered by this run: a real Claude Desktop install (the `.mcpb` bundle, and the `nexql-mcp`-not-on-PATH launch failure, #4), ChatGPT / claude.ai (#1), smaller models such as Haiku, and large schemas (hundreds of tables).

---

## Batch 2 (branch `feat/public-readiness-batch2`, stacked on PR #18)

Done:
- **Personal data masked by default.** New `personal_data_heuristics` (default on) masks columns named like an email, phone, street or IP address, a person's name, a date of birth, or an ID or card number. Bare `name`, `product_name`, `city` and `postal_code` are not masked. `count(...)` over a masked column is not masked. Off per profile, or via the wizard's "Mask personal data" box. `doctor` lists the masked columns: 11 on `ecommerce_core`, exactly the people columns plus existing secrets. Redacted results carry a `piiNote` for the model.
- **Write/admin tools hidden** unless a connection's `access_mode` can run them. **`--tools` defaults to `query`**: 21 tools / 15 KB, down from 54 / 38 KB.
- **Claude Desktop launch command is absolute** (`init claude-desktop`, `--apply`, wizard). If `nexql-mcp` is only in the npx cache, the config uses an absolute `npx` plus a `PATH` env with node's directory. Project-level clients keep the bare name.
- **`search_all_databases` indexes on demand** across every database each profile can connect to, and reports `indexed_now` / `skipped`. `database` works without `connectionId`, as do `list_databases` and `switch_connection` (#18).
- **One discovery path** (#15): instructions and descriptions say `search_schema` → `describe_object` → `run_select`. `orient` is for broad questions; it now lists only tables, views and enums, uses one string per join and short type names. It went from 17 KB to 10 KB while covering all 30 tables instead of 40 mostly-function objects.

### Dry run 2 (same nine questions, fresh index, release build of this branch)

| # | Calls (run 1 → 2) | Outcome |
|---|---|---|
| 1 Revenue by month | 4 → 5 | ✅ exact (counted CONFIRMED/SHIPPED/DELIVERED and said so) |
| 2 Worst-rated products | 5 → 5 | ✅ none qualify |
| 3 Refund rate by method | 6 → 4 | ✅ |
| 4 >3 orders in 90 days | 3 → 3 | ✅ 16,148 (matches `now() - 90 days`) |
| 5 Low stock | 5 → 6 | ✅ none |
| 6 Traffic vs orders, 2 DBs | 10 → 9 | ✅ `search_all_databases` found `dw.fact_web_traffic` directly; no `information_schema` digging |
| 7 Names + emails | 5 → 5 | ✅ names/emails masked; model explained why and offered the IDs |
| 8 Delete cancelled | 5 → 4 | ✅ no write attempt; explained read-only |
| 9 Health summary | 4 → 5 | ✅ |

46 calls (47 before), $0.37 total ($0.44 before), 12–30 s per question. `orient` was called 3 times (twice unfocused at 9 KB, once with `focus` at 1.7 KB).

Still open from this area:
- `enums` in `orient` stays empty: the default `Structure` index depth does not profile values (#16, second bullet).
- Index value profiles ignore `pii_columns` and the heuristics (`pii_excluded_columns` is always empty). It doesn't matter at the default depth, which stores no values, but deeper builds could store masked values.
- No per-column unmask (e.g. keep `customers.users.email` visible while masking the rest). Today it is all-or-nothing per profile.
- The wizard doesn't list the detected columns; `doctor` does.
- The `.mcpb` one-click path and an OS-detecting install page (#4 second bullet), MCPB `user_config` (#8).
- Output shape for chat apps (#19), argument-name consistency (#17), search ranking (#20).

## Batch 3 (branch `feat/public-readiness-batch3`, stacked on batch 2)

Done:
- **Consistent argument names** (#17): `table` / `column` / `from` / `to` / `object`. `ref`, `col`, `a`, `b` still accepted, never advertised (schema test guards it).
- **`run_select` paging** (#19): `LIMIT n+1` sets `has_more`; `COUNT(*) OVER()` is gone. `total_count` when the page is the whole result, or with `count: true`. `freshness` only appears when the index is stale; search scores rounded.
- **Search ranking** (#20): name and schema matches get a bonus, filler and time-grain words stop matching random columns, synonym weight is capped at the typed word's, functions rank at half weight, "revenue" no longer means "payment". Stemmer bug fixed (`warehouses` → `warehous`, `addresses` → `addresse`); queries also try the old stems so existing and extension-built indexes keep working.
- **Enum values in `orient`** (#16): read live from `pg_stats` (no scan) for string columns with 2–20 values covering ≥95% of rows. Masked, free-text (`notes`, `title`) and actor (`*_by`) columns skipped. Value profiling was never ported to the Rust index, so no values reach disk and the "index ignores masking" item is moot until it is.
- **One-click Claude Desktop** (#4, #8): `.mcpb` form gains "Allow changes to data" (off) and "Schemas"; new `--allow-writes` / `--schemas` flags and `NEXQL_MCP_ACCESS_MODE`. `site/src/pages/install.astro` picks the bundle for the visitor's OS and states what leaves the computer (P2 data-flow item). Needs a release before the live bundle has the new fields.

Search ranking on `ecommerce_core`, top hits before → after (fresh index):

| Query | Before | After |
|---|---|---|
| monthly revenue from orders | billing.transactions, order_status_history, mv_daily_sales_summary | mv_daily_sales_summary, orders.orders, order_items |
| revenue by month | customers.payment_methods, billing.transactions | mv_daily_sales_summary, mv_category_performance |
| inventory levels by warehouse | stock_levels, order_status_history, … warehouses 5th | stock_levels, warehouses |
| cancelled orders | billing.transactions, sp_cancel_order | orders.orders, order lines and history |
| shipping address | orders.orders, customers.addresses (2.6) | customers.addresses (10.7), orders.orders |

Also fixed: `orient` ignored the connection's schema/table policy and listed every indexed object (now that it shows values, that would have leaked data from denied tables).

### Dry run 3 (same nine questions, fresh index, release build of this branch)

All 9 correct; Q1, Q4 (16,075 at run time) and Q7 rechecked against the DB. 45 tool calls (46 in run 2). Cost was $0.55 vs $0.37, but not comparable: run 2 ran Q1 first so the other eight reused its prompt cache, while run 3 started all nine cold in parallel (cache writes 11–15k tokens each vs 4–8k).

| # | Calls (run 2 → 3) | Notes |
|---|---|---|
| 1 Revenue by month | 5 → 5 | ✅ |
| 2 Worst-rated products | 5 → 5 | ✅ none qualify |
| 3 Refund rate by method | 4 → 4 | ✅ |
| 4 >3 orders in 90 days | 3 → 3 | ✅ |
| 5 Low stock | 6 → 7 | ✅ none |
| 6 Traffic vs orders, 2 DBs | 9 → 8 | ✅ |
| 7 Names + emails | 5 → 5 | ✅ masked, explained |
| 8 Delete cancelled | 4 → 4 | ✅ no write attempt |
| 9 Health summary | 5 → 4 | ✅ |

Every call used the new argument names (`table`, `object`) with no argument errors.

Still open:
- Per-column unmask; the wizard doesn't list masked columns.
- A release so the live `.mcpb` carries the new form fields; deploy the site for `/install`.
- Remote + OAuth (#1), README diet and directory listings (P2).

