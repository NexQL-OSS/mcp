# nexql-index golden files (Phase 3)

Durable gate: Rust `build_index` against a fixed seed must match committed
artifacts under `expected/` after normalizing non-deterministic fields.

## Layout

| Path | Purpose |
| --- | --- |
| `../fixtures/seed_schema.sql` | Fixed `public.users` / `public.orders` (+ comments) |
| `expected/` | Normalized Rust builder output (committed) |
| `ts/` | Format-v1 twin of `expected/` (stand-in for TS `IndexBuilder` output until a host-free harness exists) |
| `pre_cutover/` | Same artifacts under `{root}/dbindex/golden-conn/postgres/` — extension layout for Phase 7 |

## Regenerate expected/

Needs `initdb` + `postgres` on `PATH` (or a live URL — see test for env hooks).

```bash
cd mcp
NEXQL_MCP_UPDATE_GOLDEN=1 cargo test -p nexql-index --test golden_parity -- --nocapture
```

Then commit any diffs under `expected/`.

## What is normalized

Before write/compare:

- `indexedAt` → fixed ISO timestamp
- `stats.buildMs` / `stats.queriesRun` → `0`; `stats.warnings` → `[]`
- `pgVersion` → `"GOLDEN"`
- `schemaFingerprint` → `"GOLDEN"` (presence still asserted on the live manifest)
- `oid` / `sizeBytes` / `rowEstimate` → `0`
- `objectHash` → `"GOLDEN"`
- shard `hash` / `bytes` → `"GOLDEN"` / `0`
- JSON object keys sorted recursively (HashMap postings order)

## TS / pre-cutover fixtures

`ts/` and `pre_cutover/` are kept in lockstep with `expected/` via:

```bash
./scripts/sync_pre_cutover_fixture.sh
```

Gate: `cargo test -p nexql-index --test pre_cutover_compat`.

When a host-free TS `IndexBuilder` harness exists, regenerate `ts/` from that
builder against `seed_schema.sql` and keep the byte-compare gate green.

## CI expectation

`cargo test -p nexql-index` skips the live PG integration when `initdb`/`postgres`
are missing; unit tests for `compare_normalized_manifest`, structure invariants,
and pre-cutover/TS parity always run. With Postgres available, the integration
test fails on golden drift.
