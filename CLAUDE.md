# CLAUDE.md — nexql-mcp

Rust workspace for the standalone NexQL Postgres MCP server. Implementation follows the phased roadmap in `README.md`; crates are scaffolds until their phase lands.

**Agent skill:** `.claude/skills/nexql-mcp-dev/SKILL.md` — read first for session bootstrap, phase discipline, and testing gates.

## Layout

Crate roles and one-directional layering (`policy` + `conn` → `index` → `tools` → binary): see `README.md`. `nexql-tools` must never depend on `nexql-proto`.

## Commands

```bash
cargo check --workspace
cargo run -p nexql-mcp -- doctor   # resolve + connect + session guards
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

These are what `.github/workflows/ci.yml` gates on. Clippy warnings fail CI.

**Build deps:** `pg_query` needs clang/libclang (`sudo pacman -S clang` on Arch, or `apt install clang libclang-dev` on Debian/Ubuntu). CI installs clang on ubuntu-latest.

If using a user-local LLVM (e.g. `~/.local/llvm`) without system `clang`:

```bash
export LIBCLANG_PATH=$HOME/.local/llvm/lib
export PATH=$HOME/.local/llvm/bin:$PATH
export BINDGEN_EXTRA_CLANG_ARGS="-resource-dir=$($HOME/.local/llvm/bin/clang -print-resource-dir) -I$HOME/.local/llvm/lib/clang/18/include"
# libclang 18 may need real ncurses5 ABI (not a symlink to libtinfo.so.6):
#   curl -fsSL -o /tmp/libtinfo5.deb http://deb.debian.org/debian/pool/main/n/ncurses/libtinfo5_6.4-4_amd64.deb
#   extract libtinfo.so.5* into ~/.local/lib and:
export LD_LIBRARY_PATH=$HOME/.local/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
```

## Conventions

- **Edition:** 2024, MSRV pinned in root `Cargo.toml` (`rust-version`)
- **Deps:** add to root `[workspace.dependencies]` first, then `{ workspace = true }` in the member crate. Same for `version`/`edition`/`license`/`repository`.
- **Config:** `~/.config/nexql-mcp/`, env prefix `NEXQL_MCP_*`
- **Data:** `~/.local/share/nexql-mcp/`
- **Resource URIs:** `nexql://<profile>/<database>/…` (unchanged from TS)
- **SQL validation:** `pg_query.rs` — never prefix-string checks
- **Read-only default:** `SET default_transaction_read_only = ON` on every pool connection
- **Index format:** keep compatible with `pro/src/features/dbindex/indexFormat.ts`
- **Licenses:** new deps must satisfy `deny.toml`'s allowlist. CI runs `cargo-deny`.

## Git

Feature branches (`feat/…`) → PR to `main`. CI runs on PRs to `main` only.

## Porting map

See `docs/REFERENCE.md` for TS → Rust file mapping.

## IP / licensing

GPL-3.0-only here from v0.2.0 (v0.1.6 and earlier shipped Apache-2.0 — that grant stands). Every `.rs` file carries an `SPDX-License-Identifier: GPL-3.0-only` header; keep it on new files. Pro-only features (provider embeddings, OAuth gateway, audit sinks) stay out of this repo. Do not copy proprietary strings from `nexql-pro` into free artifacts.

Copyleft is one-directional: Apache/MIT/BSD/MPL deps may flow **into** this repo, but code from here must not be copied into `nexql-pro` or any other proprietary tree. `deny.toml` keeps GPL deps out of the graph while allowing it on our own six crates.

## Before phase 2

- Verify `nexql-mcp` is free on crates.io and npm
- Prototype `rmcp` vs hand-rolled `nexql-proto` (2-day spike)
