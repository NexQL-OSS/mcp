#!/usr/bin/env bash
# Local end-to-end smoke for nexql-mcp stdio (no Docker / no Inspector required).
#
# Usage:
#   ./scripts/local_mcp_smoke.sh
#   DATABASE_URL=postgres://… ./scripts/local_mcp_smoke.sh   # reuse existing PG
#   NEXQL_MCP_BIN=./target/debug/nexql-mcp ./scripts/local_mcp_smoke.sh
#
# Optional Inspector (separate terminal, after this prints the URL):
#   npx -y @modelcontextprotocol/inspector <bin> <database_url>
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BIN="${NEXQL_MCP_BIN:-}"
if [[ -z "$BIN" ]]; then
  if [[ -x "$ROOT/target/release/nexql-mcp" ]]; then
    BIN="$ROOT/target/release/nexql-mcp"
  elif [[ -x "$ROOT/target/debug/nexql-mcp" ]]; then
    BIN="$ROOT/target/debug/nexql-mcp"
  else
    echo "error: no nexql-mcp binary — run: cargo build -p nexql-mcp --release" >&2
    exit 1
  fi
fi

INDEX_DIR="${NEXQL_MCP_INDEX_DIR:-$(mktemp -d /tmp/nexql-mcp-index.XXXXXX)}"
export NEXQL_MCP_INDEX_DIR="$INDEX_DIR"
cleanup() {
  if [[ -n "${PG_CHILD:-}" ]]; then
    kill "$PG_CHILD" 2>/dev/null || true
    wait "$PG_CHILD" 2>/dev/null || true
  fi
  if [[ -n "${TMP_PGDATA:-}" && -d "${TMP_PGDATA:-}" ]]; then
    rm -rf "$TMP_PGDATA"
  fi
}
trap cleanup EXIT

seed_smoke_schema() {
  local url="$1"
  if [[ -f "$ROOT/crates/nexql-index/tests/fixtures/seed_schema.sql" ]]; then
    psql "$url" -v ON_ERROR_STOP=1 -f "$ROOT/crates/nexql-index/tests/fixtures/seed_schema.sql" >/dev/null
  else
    psql "$url" -v ON_ERROR_STOP=1 >/dev/null <<'SQL'
CREATE TABLE public.users (
  id serial PRIMARY KEY,
  email text NOT NULL,
  name text
);
CREATE TABLE public.orders (
  id serial PRIMARY KEY,
  user_id int REFERENCES public.users(id),
  total numeric
);
SQL
  fi
}

public_has_user_tables() {
  local url="$1"
  local count
  count="$(psql "$url" -tAc \
    "SELECT count(*)::int FROM information_schema.tables WHERE table_schema = 'public' AND table_type = 'BASE TABLE'" \
    2>/dev/null || echo 0)"
  [[ "${count:-0}" -gt 0 ]]
}

start_temp_pg() {
  command -v initdb >/dev/null
  command -v postgres >/dev/null
  TMP_PGDATA="$(mktemp -d /tmp/nexql-mcp-pg.XXXXXX)"
  local port
  port="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
  initdb -D "$TMP_PGDATA" -A trust -U nexql --locale=C --encoding=UTF8 \
    >/dev/null 2>&1
  postgres -D "$TMP_PGDATA" -p "$port" \
    -c listen_addresses=127.0.0.1 \
    -c unix_socket_directories= \
    >/dev/null 2>&1 &
  PG_CHILD=$!
  local url="postgres://nexql@127.0.0.1:${port}/postgres"
  local i=0
  until psql "$url" -c 'SELECT 1' >/dev/null 2>&1; do
    i=$((i + 1))
    if [[ $i -gt 50 ]]; then
      echo "error: temp postgres failed to start" >&2
      exit 1
    fi
    sleep 0.1
  done
  seed_smoke_schema "$url"
  echo "$url"
}

if [[ -n "${DATABASE_URL:-}" ]]; then
  URL="$DATABASE_URL"
  echo "using DATABASE_URL"
  if command -v psql >/dev/null 2>&1 && ! public_has_user_tables "$URL"; then
    echo "seeding empty public schema for smoke"
    seed_smoke_schema "$URL"
  fi
else
  echo "starting throwaway Postgres (initdb)…"
  URL="$(start_temp_pg)"
  echo "temp PG: $URL"
fi

echo "binary: $BIN"
echo "index:  $INDEX_DIR"

echo "— doctor —"
"$BIN" "$URL" doctor

echo "— index build —"
"$BIN" "$URL" index build

echo "— stdio JSON-RPC smoke —"
# One line per request; server answers on stdout (logs on stderr).
RESP="$(
  {
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"local-smoke","version":"0"}}}'
    printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
    printf '%s\n' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_schemas","arguments":{}}}'
    printf '%s\n' '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"search_schema","arguments":{"query":"user"}}}'
    printf '%s\n' '{"jsonrpc":"2.0","id":5,"method":"prompts/list"}'
    printf '%s\n' '{"jsonrpc":"2.0","id":6,"method":"resources/list"}'
  } | "$BIN" "$URL" 2>/dev/null
)"

python3 - "$RESP" <<'PY'
import json, sys
raw = sys.argv[1]
lines = [l for l in raw.splitlines() if l.strip()]
by_id = {}
for line in lines:
    try:
        msg = json.loads(line)
    except json.JSONDecodeError:
        continue
    if "id" in msg:
        by_id[msg["id"]] = msg

def need(i, pred, label):
    msg = by_id.get(i)
    if not msg:
        raise SystemExit(f"FAIL: missing response id={i} ({label})")
    if "error" in msg and msg["error"]:
        raise SystemExit(f"FAIL: id={i} error={msg['error']}")
    if not pred(msg.get("result") or {}):
        raise SystemExit(f"FAIL: id={i} bad result for {label}: {msg.get('result')}")
    print(f"ok  id={i} {label}")

need(1, lambda r: r.get("protocolVersion") and "tools" in (r.get("capabilities") or {}), "initialize")
need(2, lambda r: isinstance(r.get("tools"), list) and len(r["tools"]) >= 12, "tools/list")
need(3, lambda r: True, "list_schemas")
# search may be empty if index empty, but call must succeed
need(4, lambda r: "content" in r or True, "search_schema")
need(5, lambda r: isinstance(r.get("prompts"), list) and len(r["prompts"]) >= 4, "prompts/list")
need(6, lambda r: "resources" in r, "resources/list")
print(f"tools advertised: {len(by_id[2]['result']['tools'])}")
print(f"prompts: {len(by_id[5]['result']['prompts'])}")
PY

echo
echo "local smoke: ok"
echo
echo "MCP Inspector (interactive):"
echo "  npx -y @modelcontextprotocol/inspector $BIN \"$URL\""
echo "Cursor / Claude config:"
echo "  $BIN init cursor \"$URL\""
