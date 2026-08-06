#!/usr/bin/env bash
# Package a platform build of nexql-mcp as an MCPB bundle (mcpb/manifest.json + binary,
# zipped to .mcpb — the Claude Desktop one-click install format).
#
# Usage:
#   ./scripts/package-mcpb.sh <vendor> <source-binary> <output-dir>
#
# Example:
#   ./scripts/package-mcpb.sh linux-x64 target/release/nexql-mcp dist/mcpb
#
# Requires: jq, zip.
set -euo pipefail

VENDOR="${1:?vendor required (e.g. linux-x64, darwin-arm64, win32-x64)}"
SRC="${2:?source binary required}"
OUT_DIR="${3:?output dir required}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MCP_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MANIFEST="$MCP_ROOT/mcpb/manifest.json"

test -f "$SRC"
test -f "$MANIFEST"

case "$VENDOR" in
  win32-x64) BIN_NAME="nexql-mcp.exe" ;;
  linux-x64|linux-arm64|darwin-x64|darwin-arm64) BIN_NAME="nexql-mcp" ;;
  *)
    echo "unknown vendor: $VENDOR" >&2
    exit 1
    ;;
esac

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

mkdir -p "$STAGE/server"
cp "$SRC" "$STAGE/server/$BIN_NAME"
chmod +x "$STAGE/server/$BIN_NAME" 2>/dev/null || true

# Patch entry_point / mcp_config.command to the platform-specific binary name
# (manifest.json's checked-in default is the unix name).
jq --arg entry "server/$BIN_NAME" --arg cmd "\${__dirname}/server/$BIN_NAME" \
  '.server.entry_point = $entry | .server.mcp_config.command = $cmd' \
  "$MANIFEST" > "$STAGE/manifest.json"

mkdir -p "$OUT_DIR"
OUT_FILE="$OUT_DIR/nexql-mcp-${VENDOR}.mcpb"
( cd "$STAGE" && zip -qr "$OUT_FILE" manifest.json server )

sha256sum "$OUT_FILE" > "$OUT_FILE.sha256" 2>/dev/null \
  || shasum -a 256 "$OUT_FILE" > "$OUT_FILE.sha256"

echo "Packaged $SRC -> $OUT_FILE"
