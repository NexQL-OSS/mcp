#!/usr/bin/env bash
# Copy a built nexql-mcp binary into the matching npm platform package stub.
#
# Usage:
#   ./scripts/package-npm-platform.sh <vendor-dir> <source-binary>
#
# Example:
#   ./scripts/package-npm-platform.sh linux-x64 target/x86_64-unknown-linux-gnu/release/nexql-mcp
set -euo pipefail

VENDOR="${1:?vendor dir required (e.g. linux-x64)}"
SRC="${2:?source binary required}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MCP_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

case "$VENDOR" in
  linux-x64) PKG="mcp-linux-x64"; BIN="nexql-mcp" ;;
  linux-arm64) PKG="mcp-linux-arm64"; BIN="nexql-mcp" ;;
  darwin-x64) PKG="mcp-darwin-x64"; BIN="nexql-mcp" ;;
  darwin-arm64) PKG="mcp-darwin-arm64"; BIN="nexql-mcp" ;;
  win32-x64) PKG="mcp-win32-x64"; BIN="nexql-mcp.exe" ;;
  *)
    echo "unknown vendor dir: $VENDOR" >&2
    exit 1
    ;;
esac

DEST_DIR="$MCP_ROOT/npm/packages/$PKG/bin"
mkdir -p "$DEST_DIR"
cp -f "$SRC" "$DEST_DIR/$BIN"
chmod +x "$DEST_DIR/$BIN" 2>/dev/null || true

echo "Packaged $SRC → $DEST_DIR/$BIN"
