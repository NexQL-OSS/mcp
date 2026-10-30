#!/usr/bin/env bash
# Sync formatVersion=1 golden artifacts into the pre-cutover dbindex layout and
# the `ts/` twin used by the cross-lang parity gate.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXPECTED="$ROOT/crates/nexql-index/tests/golden/expected"
TS="$ROOT/crates/nexql-index/tests/golden/ts"
PRE="$ROOT/crates/nexql-index/tests/golden/pre_cutover/dbindex/golden-conn/postgres"

mkdir -p "$TS" "$PRE"
cp -f "$EXPECTED"/* "$TS/"
# pre_cutover/ stands for an index the VS Code extension wrote, so its
# tokens.json keeps the TS stemmer's output ("purchas"). Queries must keep
# finding objects in such indexes; see legacy_stem_word in lexical.rs.
for f in "$EXPECTED"/*; do
  [ "$(basename "$f")" = "tokens.json" ] && continue
  cp -f "$f" "$PRE/"
done
echo "Synced expected/ → ts/ and pre_cutover/dbindex/golden-conn/postgres/ (except tokens.json)"
