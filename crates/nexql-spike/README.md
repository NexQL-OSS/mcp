# nexql-spike (Phase 0 — throwaway)

Proof that the two biggest unknowns are tractable: **tokio-postgres catalog queries** and **candle MiniLM** embeddings. Not shipped; excluded from workspace `default-members`.

## Run

```bash
# unit + integration (spins a throwaway PG via initdb when no URL set)
cargo test -p nexql-spike

# metrics harness (downloads MiniLM on first run into HF cache)
cargo run --release -p nexql-spike

# cold-start probe (no model load)
cargo run --release -p nexql-spike -- --version
```

Optional: `NEXQL_MCP_SPIKE_DATABASE_URL` / `DATABASE_URL` to reuse an existing Postgres instead of `initdb`.

## Measured (2026-07-22, Linux x64 WSL2, release)

| Metric | Value | Notes |
|--------|-------|-------|
| Release binary size | **13.4 MB** | `<30 MB` gate ✓ |
| Cold start (`--version`) | **~4–5 ms** | product budget `<20 ms` (Phase 6) ✓ directionally |
| Model load (cached HF hub) | **~140–250 ms** | `sentence-transformers/all-MiniLM-L6-v2` |
| Embed 100 object strings | **~1.7 s** (~17 ms/obj) | CPU, mean-pool + L2 |
| RSS with model loaded | **~103 MB** | expected; product idle RSS `<25 MB` is without candle resident |
| Catalog sample | relations=2, columns=6, fks=1 | seeded `public.users` → `public.orders` |

## What this killed

| Unknown | Result |
|---------|--------|
| `tokio-postgres` + catalog SQL from TS | Works; bind `oid[]` as `Vec<u32>` |
| `rustls` path (`sslmode=require`) | Compiles via `tokio-postgres-rustls` (manual Neon URL smoke deferred) |
| `candle` MiniLM load/link | Works on CPU; dim=384; no NaN/Inf |
| Cosine rank "user email" → `public.users.email` | Top-3 ✓ |

## Exit

Phase 0 complete. Keep this crate until Phase 5 reuses the embed path, then delete or fold into `nexql-index::embed`.
