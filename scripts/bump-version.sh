#!/usr/bin/env bash
# Atomic version bump script across all repository manifests.
#
# Usage:
#   ./scripts/bump-version.sh 0.2.0
#   ./scripts/bump-version.sh --check [VERSION]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <new-version> or $0 --check [version]" >&2
  exit 1
fi

CHECK_MODE=false
if [[ "$1" == "--check" ]]; then
  CHECK_MODE=true
  shift
fi

TARGET_VERSION="${1:-}"
if [[ -z "$TARGET_VERSION" ]]; then
  TARGET_VERSION="$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)"
fi

echo "Target version: $TARGET_VERSION"

NPM_MANIFESTS=(
  "npm/package.json"
  "npm/packages/mcp-darwin-arm64/package.json"
  "npm/packages/mcp-darwin-x64/package.json"
  "npm/packages/mcp-linux-arm64/package.json"
  "npm/packages/mcp-linux-x64/package.json"
  "npm/packages/mcp-win32-x64/package.json"
)

MCPB_MANIFEST="mcpb/manifest.json"
SERVER_JSON="server.json"

if [[ "$CHECK_MODE" == "true" ]]; then
  ERRORS=0

  # Check root Cargo.toml workspace version
  ws_version="$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)"
  if [[ "$ws_version" != "$TARGET_VERSION" ]]; then
    echo "MISMATCH in Cargo.toml workspace version: found $ws_version, expected $TARGET_VERSION" >&2
    ERRORS=$((ERRORS + 1))
  fi

  # Check internal workspace path-dep pins in root Cargo.toml
  pins="$(grep -E 'nexql-[a-z]+ = \{ path = .* version = "' Cargo.toml | cut -d'"' -f4)"
  for pin in $pins; do
    if [[ "$pin" != "$TARGET_VERSION" ]]; then
      echo "MISMATCH in Cargo.toml path-dep pin: found $pin, expected $TARGET_VERSION" >&2
      ERRORS=$((ERRORS + 1))
    fi
  done

  # Check NPM manifests (top-level version field)
  for mf in "${NPM_MANIFESTS[@]}"; do
    if [[ -f "$mf" ]]; then
      v="$(grep -m1 '"version"' "$mf" | cut -d'"' -f4)"
      if [[ "$v" != "$TARGET_VERSION" ]]; then
        echo "MISMATCH in $mf: found $v, expected $TARGET_VERSION" >&2
        ERRORS=$((ERRORS + 1))
      fi
    fi
  done

  # Check npm optionalDependencies pins match release version
  if [[ -f "npm/package.json" ]]; then
    while IFS= read -r pin; do
      if [[ "$pin" != "$TARGET_VERSION" ]]; then
        echo "MISMATCH in npm/package.json optionalDependencies: found $pin, expected $TARGET_VERSION" >&2
        ERRORS=$((ERRORS + 1))
      fi
    done < <(grep -E '"@nexql/mcp-[^"]+": "[^"]+"' npm/package.json | cut -d'"' -f4)
  fi

  # Check MCPB manifest
  if [[ -f "$MCPB_MANIFEST" ]]; then
    v="$(grep -m1 '"version"' "$MCPB_MANIFEST" | cut -d'"' -f4)"
    if [[ "$v" != "$TARGET_VERSION" ]]; then
      echo "MISMATCH in $MCPB_MANIFEST: found $v, expected $TARGET_VERSION" >&2
      ERRORS=$((ERRORS + 1))
    fi
  fi

  # Check MCP Registry server.json
  if [[ -f "$SERVER_JSON" ]]; then
    v="$(python3 -c "import json; print(json.load(open('$SERVER_JSON'))['version'])")"
    if [[ "$v" != "$TARGET_VERSION" ]]; then
      echo "MISMATCH in $SERVER_JSON top-level version: found $v, expected $TARGET_VERSION" >&2
      ERRORS=$((ERRORS + 1))
    fi
    while IFS= read -r pin; do
      if [[ "$pin" != "$TARGET_VERSION" ]]; then
        echo "MISMATCH in $SERVER_JSON package version/tag: found $pin, expected $TARGET_VERSION" >&2
        ERRORS=$((ERRORS + 1))
      fi
    done < <(python3 - <<PY
import json
d = json.load(open("$SERVER_JSON"))
for pkg in d.get("packages", []):
    if "version" in pkg:
        print(pkg["version"])
    ident = pkg.get("identifier", "")
    if ":" in ident and ident.startswith("ghcr.io/"):
        print(ident.rsplit(":", 1)[-1])
PY
)
  fi

  if [[ $ERRORS -gt 0 ]]; then
    echo "Version check FAILED with $ERRORS mismatches." >&2
    exit 1
  else
    echo "All manifest versions match: $TARGET_VERSION"
    exit 0
  fi
fi

# Update root Cargo.toml workspace version
sed -i -E "s/^version = \"[^\"]+\"/version = \"$TARGET_VERSION\"/" Cargo.toml

# Update Cargo.toml workspace path-dep pins
sed -i -E "s/(nexql-[a-z]+ = \{ path = \"[^\"]+\", version = \")[^\"]+(\")/\1$TARGET_VERSION\2/g" Cargo.toml

# Update NPM package manifests (top-level version)
for mf in "${NPM_MANIFESTS[@]}"; do
  if [[ -f "$mf" ]]; then
    # Only replace the package's own "version" field (first occurrence), not optionalDeps.
    python3 - <<PY
import json
from pathlib import Path
p = Path("$mf")
data = json.loads(p.read_text())
data["version"] = "$TARGET_VERSION"
if "optionalDependencies" in data:
    data["optionalDependencies"] = {
        k: "$TARGET_VERSION" for k in data["optionalDependencies"]
    }
p.write_text(json.dumps(data, indent=2) + "\n")
PY
  fi
done

# Update MCPB manifest
if [[ -f "$MCPB_MANIFEST" ]]; then
  python3 - <<PY
import json
from pathlib import Path
p = Path("$MCPB_MANIFEST")
data = json.loads(p.read_text())
data["version"] = "$TARGET_VERSION"
p.write_text(json.dumps(data, indent=2) + "\n")
PY
fi

# Update MCP Registry server.json
if [[ -f "$SERVER_JSON" ]]; then
  python3 - <<PY
import json
from pathlib import Path
p = Path("$SERVER_JSON")
data = json.loads(p.read_text())
data["version"] = "$TARGET_VERSION"
for pkg in data.get("packages", []):
    if "version" in pkg:
        pkg["version"] = "$TARGET_VERSION"
    ident = pkg.get("identifier", "")
    if ident.startswith("ghcr.io/") and ":" in ident:
        pkg["identifier"] = ident.rsplit(":", 1)[0] + ":$TARGET_VERSION"
p.write_text(json.dumps(data, indent=2) + "\n")
PY
fi

echo "Successfully bumped all manifest versions to $TARGET_VERSION."
