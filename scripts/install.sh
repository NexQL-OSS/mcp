#!/usr/bin/env bash
# Install nexql-mcp from GitHub Releases (Linux / macOS).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/NexQL-OSS/mcp/main/scripts/install.sh | bash
#
# Optional env:
#   NEXQL_MCP_VERSION=v0.2.1     pin a release (default: latest)
#   NEXQL_MCP_INSTALL_DIR=/path    install directory (default: /usr/local/bin or ~/.local/bin)
#   NEXQL_MCP_REPO=NexQL-OSS/mcp  GitHub owner/repo
set -euo pipefail

REPO="${NEXQL_MCP_REPO:-NexQL-OSS/mcp}"
INSTALL_DIR="${NEXQL_MCP_INSTALL_DIR:-}"
TAG="${NEXQL_MCP_VERSION:-}"

info() { printf '==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

detect_triple() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "${os}-${arch}" in
    Linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
    Linux-aarch64|Linux-arm64) echo "aarch64-unknown-linux-gnu" ;;
    Darwin-x86_64) echo "x86_64-apple-darwin" ;;
    Darwin-arm64) echo "aarch64-apple-darwin" ;;
    *)
      die "unsupported platform: ${os} ${arch} — see https://github.com/${REPO}/releases"
      ;;
  esac
}

resolve_tag() {
  if [[ -n "$TAG" ]]; then
    printf '%s\n' "$TAG"
    return
  fi
  need_cmd curl
  TAG="$(
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
      | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' \
      | head -n1
  )"
  [[ -n "$TAG" ]] || die "could not resolve latest release tag"
  printf '%s\n' "$TAG"
}

pick_install_dir() {
  if [[ -n "$INSTALL_DIR" ]]; then
    printf '%s\n' "$INSTALL_DIR"
    return
  fi
  if [[ -w /usr/local/bin ]] 2>/dev/null; then
    printf '%s\n' "/usr/local/bin"
    return
  fi
  if command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
    printf '%s\n' "/usr/local/bin"
    return
  fi
  printf '%s\n' "${HOME}/.local/bin"
}

install_binary() {
  local triple tag stage archive url tmpdir dest bin stage_dir
  need_cmd curl
  need_cmd tar
  need_cmd install

  triple="$(detect_triple)"
  tag="$(resolve_tag)"
  dest="$(pick_install_dir)"
  stage="nexql-mcp-${tag}-${triple}"
  archive="${stage}.tar.gz"
  url="https://github.com/${REPO}/releases/download/${tag}/${archive}"

  tmpdir="$(mktemp -d)"
  trap "rm -rf '${tmpdir}'" EXIT

  info "Installing nexql-mcp ${tag} for ${triple}"
  info "Downloading ${url}"
  curl -fsSL -o "${tmpdir}/${archive}" "$url"
  tar -xzf "${tmpdir}/${archive}" -C "$tmpdir"
  stage_dir="${tmpdir}/${stage}"
  bin="${stage_dir}/nexql-mcp"
  [[ -f "$bin" ]] || die "archive did not contain ${stage}/nexql-mcp"

  mkdir -p "$dest"
  if [[ "$dest" == "/usr/local/bin" ]] && [[ ! -w "$dest" ]]; then
    need_cmd sudo
    sudo install -m 0755 "$bin" "${dest}/nexql-mcp"
  else
    install -m 0755 "$bin" "${dest}/nexql-mcp"
  fi

  print_next_steps "$dest"
}

print_next_steps() {
  local dest="$1"
  local path_hint=""

  case ":${PATH}:" in
    *":${dest}:"*) ;;
    *)
      path_hint="${dest}"
      warn "${dest} is not on your PATH"
      ;;
  esac

  cat <<EOF

nexql-mcp installed successfully.

EOF

  if [[ -n "$path_hint" ]]; then
    cat <<EOF
Add it to your PATH (add to ~/.bashrc or ~/.zshrc):

  export PATH="${path_hint}:\$PATH"

EOF
  fi

  cat <<'EOF'
Next steps:

  1. Verify the install:
       nexql-mcp --version

  2. Test your Postgres connection:
       nexql-mcp postgres://USER:PASS@localhost:5432/DBNAME doctor

  3. Wire an MCP client (pick one):
       nexql-mcp init cursor
       nexql-mcp init claude-desktop
       nexql-mcp init vscode-copilot

     Or run the guided setup wizard:
       nexql-mcp tui

     Installed via uv? Same commands after `uv tool install nexql-mcp`.

  Docs: https://github.com/NexQL-OSS/mcp/blob/main/docs/clients/README.md
EOF
}

install_binary
