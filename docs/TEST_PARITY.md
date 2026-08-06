# MCP test parity (TS → Rust)

Checklist mapped from `pro/test/McpServer.test.ts` (pre-cutover HTTP suite).
After Phase 7, extension tests cover stdio definition wiring only; protocol/tool
coverage lives in this repo.

| TS describe block | Rust / extension home | Status |
|-------------------|----------------------|--------|
| McpDefinitionProvider | `pro/test/McpServer.test.ts` (stdio) | Ported |
| Security (401, 405, 400, 413, rate limit) | `nexql-proto` HTTP + deferred sessions | Partial (bearer + body cap; sessions/rate-limit TBD) |
| Protocol & Tool Dispatching | `nexql-tools` + `scripts/local_mcp_smoke.sh` | Covered |
| Spec Hardening (session/DELETE/LRU) | Deferred with HTTP sessions | Open |
| Resources & Prompts | `nexql-tools` resources/prompts | Covered |
| Monitoring Tools | `nexql-tools` exec | Covered |
| Pre-cutover index layout | `nexql-index` `pre_cutover_compat` | Covered |
| Write/admin SQL corpus (Phase 9) | `nexql-policy/tests/fixtures/sql_write_corpus.toml` (42 cases) | Covered |
| Golden index format parity | `nexql-index` `golden_parity` — format-v1 vs. committed `expected/` fixtures, **not** TS `IndexBuilder` byte-compare | Covered (scope: see `tests/golden/README.md`) |
