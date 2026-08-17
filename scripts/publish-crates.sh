#!/usr/bin/env bash
# Publish workspace crates to crates.io in dependency order, waiting for each
# version to appear in the index before publishing dependents.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

USER_AGENT="${CARGO_PUBLISH_USER_AGENT:-nexql-mcp-release-ci (github.com/NexQL-OSS/mcp)}"
POLL_INTERVAL_SECS="${CARGO_PUBLISH_POLL_INTERVAL_SECS:-15}"
POLL_MAX_ATTEMPTS="${CARGO_PUBLISH_POLL_MAX_ATTEMPTS:-40}" # 40 * 15s = 10 min
PUBLISH_RETRIES="${CARGO_PUBLISH_RETRIES:-3}"

version="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
crates=(nexql-policy nexql-proto nexql-conn nexql-index nexql-tools nexql-mcp)

crate_status() {
  local crate="$1"
  local ver="$2"
  curl -s -o /dev/null -w '%{http_code}' \
    -A "$USER_AGENT" \
    "https://crates.io/api/v1/crates/${crate}/${ver}"
}

wait_for_crate() {
  local crate="$1"
  local ver="$2"
  local attempt=1
  while [ "$attempt" -le "$POLL_MAX_ATTEMPTS" ]; do
    local status
    status="$(crate_status "$crate" "$ver")"
    if [ "$status" = "200" ]; then
      echo "available: ${crate} ${ver} on crates.io (attempt ${attempt})"
      return 0
    fi
    echo "waiting: ${crate} ${ver} not indexed yet (HTTP ${status}, attempt ${attempt}/${POLL_MAX_ATTEMPTS})"
    sleep "$POLL_INTERVAL_SECS"
    attempt=$((attempt + 1))
  done
  echo "error: timed out waiting for ${crate} ${ver} on crates.io" >&2
  return 1
}

publish_once() {
  local crate="$1"
  local output
  local exit_code=0
  set +e
  output="$(cargo publish -p "$crate" --locked 2>&1)"
  exit_code=$?
  set -e
  printf '%s\n' "$output"
  if [ "$exit_code" -eq 0 ]; then
    return 0
  fi
  # Upload often succeeds even when cargo's internal index wait times out.
  if printf '%s\n' "$output" | grep -Fq "timed out waiting for"; then
    echo "note: cargo publish upload finished but index wait timed out; polling crates.io..."
    return 0
  fi
  if printf '%s\n' "$output" | grep -Fq "already exists on crates.io"; then
    return 0
  fi
  return "$exit_code"
}

publish_crate() {
  local crate="$1"
  local status
  status="$(crate_status "$crate" "$version")"
  if [ "$status" = "200" ]; then
    echo "skip: ${crate} ${version} already on crates.io"
    return 0
  fi

  local attempt=1
  while [ "$attempt" -le "$PUBLISH_RETRIES" ]; do
    echo "publish: ${crate} ${version} (attempt ${attempt}/${PUBLISH_RETRIES})"
    if publish_once "$crate"; then
      wait_for_crate "$crate" "$version"
      return 0
    fi
    echo "warning: cargo publish failed for ${crate} (attempt ${attempt})" >&2
    # If another job already published while we were uploading, treat as success.
    status="$(crate_status "$crate" "$version")"
    if [ "$status" = "200" ]; then
      echo "skip: ${crate} ${version} appeared on crates.io after publish failure"
      return 0
    fi
    attempt=$((attempt + 1))
    sleep "$POLL_INTERVAL_SECS"
  done

  echo "error: failed to publish ${crate} ${version}" >&2
  return 1
}

for crate in "${crates[@]}"; do
  publish_crate "$crate"
done

echo "all crates published for ${version}"
