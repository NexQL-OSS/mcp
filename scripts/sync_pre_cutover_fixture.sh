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
cp -f "$EXPECTED"/* "$PRE/"
echo "Synced expected/ → ts/ and pre_cutover/dbindex/golden-conn/postgres/"
