# nexql-index golden files (Phase 3)

Durable gate: Rust `build_index` against a fixed seed must match committed
artifacts under `expected/` after normalizing non-deterministic fields.

## Layout

| Path | Purpose |
| --- | --- |
| `../fixtures/seed_schema.sql` | Fixed `public.users` / `public.orders` (+ comments) |
| `expected/` | Normalized Rust builder output (committed) |
| `ts/` | Reserved for future TS `IndexBuilder` golden (not required yet) |

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

## Adding a TS golden later

1. Run the VS Code / pro `IndexBuilder` against the same `seed_schema.sql`.
2. Copy output into `ts/` (same filenames as `expected/`).
3. Wire a cross-lang compare that runs both through the same normalizer, then
   byte-compares. Defer until the TS harness does not require a full VS Code host.

## CI expectation

`cargo test -p nexql-index` skips the live PG integration when `initdb`/`postgres`
are missing; unit tests for `compare_normalized_manifest` and structure invariants
always run. With Postgres available, the integration test fails on golden drift.
