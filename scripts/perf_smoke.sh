#!/usr/bin/env bash
# Phase 6 perf smoke for nexql-mcp.
#
# Budgets
# -------
# Local cold start (`--version`):  <20 ms   (product target)
# CI cold start (GITHUB_ACTIONS):  <100 ms  (noisy shared VMs)
# Release binary size:             <30 MB
#
# Idle RSS (<25 MB) needs a live stdio server + connection; not gated here.
# Measure locally with e.g.:
#   /usr/bin/time -v timeout 2 target/release/nexql-mcp postgres://… </dev/null
#   # or: ps -o rss= -p <pid> while the server is idle on stdio
#
# search_schema p95 warm (<5 ms) deferred to criterion benches on a golden
# IndexStore fixture (no PG) — not part of this smoke script.
#
# Usage:
#   cargo build -p nexql-mcp --release
#   ./scripts/perf_smoke.sh
#   PERF_BIN=/path/to/nexql-mcp ./scripts/perf_smoke.sh
#
# Env:
#   PERF_BIN              path to release binary (default: target/release/nexql-mcp)
#   PERF_ENFORCE_LOCAL    if 1, fail when local cold start exceeds 20 ms (default: warn only)
#   GITHUB_ACTIONS        set by GHA; selects the 100 ms CI cold-start gate
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

readonly LOCAL_COLD_START_MS=20
readonly CI_COLD_START_MS=100
readonly MAX_BINARY_BYTES=$((30 * 1024 * 1024))
readonly COLD_START_SAMPLES=5

BIN="${PERF_BIN:-$ROOT/target/release/nexql-mcp}"

if [[ ! -x "$BIN" ]]; then
  echo "error: missing executable binary: $BIN" >&2
  echo "hint: cargo build -p nexql-mcp --release" >&2
  exit 1
fi

# --- binary size ---
size_bytes="$(stat -c%s "$BIN" 2>/dev/null || stat -f%z "$BIN")"
size_mb="$(awk -v b="$size_bytes" 'BEGIN { printf "%.2f", b / (1024 * 1024) }')"
echo "binary size: ${size_mb} MiB (${size_bytes} bytes)  budget=<30 MiB"

if (( size_bytes > MAX_BINARY_BYTES )); then
  echo "error: binary size ${size_bytes} exceeds ${MAX_BINARY_BYTES} bytes" >&2
  exit 1
fi

# --- cold start (`--version`) ---
# Discard first run (page cache / loader warmup), then take the min of remaining samples.
"$BIN" --version >/dev/null 2>&1 || true

min_ms=""
for _ in $(seq 1 "$COLD_START_SAMPLES"); do
  start_ns="$(date +%s%N)"
  "$BIN" --version >/dev/null
  end_ns="$(date +%s%N)"
  elapsed_ms="$(awk -v s="$start_ns" -v e="$end_ns" 'BEGIN { printf "%.3f", (e - s) / 1e6 }')"
  echo "  cold start sample: ${elapsed_ms} ms"
  if [[ -z "$min_ms" ]] || awk -v a="$elapsed_ms" -v b="$min_ms" 'BEGIN { exit !(a < b) }'; then
    min_ms="$elapsed_ms"
  fi
done

echo "cold start (min of ${COLD_START_SAMPLES}): ${min_ms} ms"

if [[ -n "${GITHUB_ACTIONS:-}" ]]; then
  budget_ms="$CI_COLD_START_MS"
  echo "CI gate: fail if cold start > ${budget_ms} ms (local product target remains ${LOCAL_COLD_START_MS} ms)"
  if awk -v m="$min_ms" -v b="$budget_ms" 'BEGIN { exit !(m > b) }'; then
    echo "error: cold start ${min_ms} ms exceeds CI budget ${budget_ms} ms" >&2
    exit 1
  fi
else
  echo "local target: <${LOCAL_COLD_START_MS} ms (CI enforces <${CI_COLD_START_MS} ms)"
  if awk -v m="$min_ms" -v b="$LOCAL_COLD_START_MS" 'BEGIN { exit !(m > b) }'; then
    if [[ "${PERF_ENFORCE_LOCAL:-0}" == "1" ]]; then
      echo "error: cold start ${min_ms} ms exceeds local budget ${LOCAL_COLD_START_MS} ms" >&2
      exit 1
    fi
    echo "warn: cold start ${min_ms} ms exceeds local target ${LOCAL_COLD_START_MS} ms (set PERF_ENFORCE_LOCAL=1 to fail)" >&2
  fi
fi

# --- RSS guidance (local-only; not measured here) ---
cat <<'EOF'

idle RSS: not measured by this script (needs a connected stdio server).
  product budget: <25 MB without candle embeddings resident
  local check: start the server against a real URL, then inspect RSS via
    /usr/bin/time -v …   or   ps -o rss= -p <pid>
search_schema warm p95 (<5 ms): deferred to criterion benches on golden fixtures.
EOF

echo "perf_smoke: ok"
