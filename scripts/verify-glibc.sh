#!/usr/bin/env bash
# Verify a Linux ELF binary's highest GLIBC symbol requirement is <= MAX_GLIBC.
#
# Release CI builds on Ubuntu 22.04 (glibc 2.35) so prebuilt binaries run on
# Debian 12, Ubuntu 22.04+, RHEL 9, and similar distros — not only 24.04+.
#
# Usage:
#   ./scripts/verify-glibc.sh path/to/nexql-mcp [max_glibc]
# Default max: 2.35
set -euo pipefail

BIN="${1:?binary path required}"
MAX="${2:-2.35}"

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

[[ -f "$BIN" ]] || die "not a file: $BIN"

if ! file "$BIN" | grep -q ELF; then
  printf 'skip: not ELF (%s)\n' "$BIN"
  exit 0
fi

command -v objdump >/dev/null 2>&1 || die "missing required command: objdump"

max_required="$(
  objdump -T "$BIN" 2>/dev/null \
    | sed -n 's/.*GLIBC_\([0-9.]*\).*/\1/p' \
    | sort -V \
    | tail -1
)"

if [[ -z "$max_required" ]]; then
  printf 'warn: no GLIBC version symbols found in %s\n' "$BIN"
  exit 0
fi

printf 'max GLIBC required: %s (limit: %s)\n' "$max_required" "$MAX"

# Return 0 when $1 > $2 (strictly greater).
ver_gt() {
  local a="$1" b="$2"
  [[ "$(printf '%s\n%s\n' "$a" "$b" | sort -V | tail -1)" == "$a" && "$a" != "$b" ]]
}

if ver_gt "$max_required" "$MAX"; then
  die "binary requires GLIBC_${max_required} > ${MAX} (portability failure)"
fi

printf 'ok: GLIBC_%s <= %s\n' "$max_required" "$MAX"
