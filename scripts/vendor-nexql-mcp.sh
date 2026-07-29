#!/usr/bin/env bash
# Vendor the nexql-mcp binary into the VS Code extension tree for package-pro.
#
# Usage:
#   ./scripts/vendor-nexql-mcp.sh [source-binary] [dest-root]
#
# Defaults:
#   source = mcp/target/release/nexql-mcp (or debug)
#   dest   = core/bin/nexql-mcp/<platform-arch>/
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OSS_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
MCP_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PLATFORM="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ARCH=x64 ;;
  aarch64|arm64) ARCH=arm64 ;;
esac
case "$PLATFORM" in
  darwin|linux) ;;
  mingw*|msys*|cygwin*) PLATFORM=win32 ;;
  *) echo "unsupported platform: $PLATFORM" >&2; exit 1 ;;
esac

BIN_NAME="nexql-mcp"
if [[ "$PLATFORM" == "win32" ]]; then
  BIN_NAME="nexql-mcp.exe"
fi

SRC="${1:-}"
if [[ -z "$SRC" ]]; then
  if [[ -x "$MCP_ROOT/target/release/nexql-mcp" ]]; then
    SRC="$MCP_ROOT/target/release/nexql-mcp"
  elif [[ -x "$MCP_ROOT/target/debug/nexql-mcp" ]]; then
    SRC="$MCP_ROOT/target/debug/nexql-mcp"
  else
    echo "No nexql-mcp binary found. Build with: (cd mcp && cargo build --release -p nexql-mcp)" >&2
    exit 1
  fi
fi

DEST_ROOT="${2:-$OSS_ROOT/core/bin/nexql-mcp}"
DEST_DIR="$DEST_ROOT/${PLATFORM}-${ARCH}"
mkdir -p "$DEST_DIR"
cp -f "$SRC" "$DEST_DIR/$BIN_NAME"
chmod +x "$DEST_DIR/$BIN_NAME"

# Placeholder README so empty platforms are documented in git.
cat > "$DEST_ROOT/README.md" <<'EOF'
# Vendored nexql-mcp binaries

Layout: `bin/nexql-mcp/<platform>-<arch>/nexql-mcp[.exe]`

Populate via:

```bash
cd mcp && cargo build --release -p nexql-mcp
./scripts/vendor-nexql-mcp.sh
```

CI release builds should vendor per target into the matching platform directory
before `make package-pro`.
EOF

# Keep other platforms as placeholders so the tree is predictable.
for pair in darwin-arm64 darwin-x64 linux-x64 linux-arm64 win32-x64; do
  mkdir -p "$DEST_ROOT/$pair"
  if [[ ! -e "$DEST_ROOT/$pair/.gitkeep" && ! -e "$DEST_ROOT/$pair/$BIN_NAME" && ! -e "$DEST_ROOT/$pair/nexql-mcp.exe" ]]; then
    touch "$DEST_ROOT/$pair/.gitkeep"
  fi
done

echo "Vendored $SRC → $DEST_DIR/$BIN_NAME"
ls -la "$DEST_DIR/$BIN_NAME"
