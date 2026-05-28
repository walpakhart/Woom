#!/usr/bin/env bash
# Fetch the upstream RTK release binary for a target triple and place it
# at `apps/desktop/src-tauri/binaries/rtk-<triple>`.
#
# Idempotent: if the destination file exists AND the embedded version
# string matches $RTK_VERSION, skip the download. Pass --force to redo
# the download anyway.
#
# Usage:
#   ./fetch-rtk.sh                  # host arch, default version
#   RTK_VERSION=v0.42.0 ./fetch-rtk.sh
#   ./fetch-rtk.sh --target aarch64-apple-darwin --force
#   ./fetch-rtk.sh --all            # fetch every supported triple

set -euo pipefail

RTK_VERSION=${RTK_VERSION:-v0.42.0}
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
BIN_DIR=$(cd "$SCRIPT_DIR/.." && pwd)/binaries
mkdir -p "$BIN_DIR"

FORCE=0
TARGETS=()
FETCH_ALL=0

while [ $# -gt 0 ]; do
  case "$1" in
    --force) FORCE=1 ;;
    --target) shift; TARGETS+=("$1") ;;
    --all)   FETCH_ALL=1 ;;
    -h|--help)
      echo "Usage: $0 [--target <triple>] [--force] [--all]"
      exit 0
      ;;
    *)
      echo "[fetch-rtk] unknown arg: $1" >&2
      exit 2
      ;;
  esac
  shift
done

ALL_TARGETS=(
  "aarch64-apple-darwin"
  "x86_64-apple-darwin"
  "x86_64-unknown-linux-musl"
  "aarch64-unknown-linux-gnu"
)

detect_host_triple() {
  local kernel arch
  kernel=$(uname -s)
  arch=$(uname -m)
  case "$kernel-$arch" in
    Darwin-arm64)  echo "aarch64-apple-darwin" ;;
    Darwin-x86_64) echo "x86_64-apple-darwin" ;;
    Linux-x86_64)  echo "x86_64-unknown-linux-musl" ;;
    Linux-aarch64) echo "aarch64-unknown-linux-gnu" ;;
    *) echo "" ;;
  esac
}

if [ "$FETCH_ALL" -eq 1 ]; then
  TARGETS=("${ALL_TARGETS[@]}")
elif [ ${#TARGETS[@]} -eq 0 ]; then
  host=$(detect_host_triple)
  if [ -z "$host" ]; then
    echo "[fetch-rtk] could not detect host triple (uname -s/-m mismatch)" >&2
    exit 2
  fi
  TARGETS=("$host")
fi

archive_name() {
  case "$1" in
    *-apple-darwin)              echo "rtk-$1.tar.gz" ;;
    x86_64-unknown-linux-musl)   echo "rtk-$1.tar.gz" ;;
    aarch64-unknown-linux-gnu)   echo "rtk-$1.tar.gz" ;;
    *)                           echo "" ;;
  esac
}

fetch_one() {
  local triple=$1
  local archive
  archive=$(archive_name "$triple")
  if [ -z "$archive" ]; then
    echo "[fetch-rtk] unsupported triple: $triple" >&2
    return 1
  fi

  local dest="$BIN_DIR/rtk-$triple"
  if [ "$FORCE" -eq 0 ] && [ -x "$dest" ]; then
    # Skip if already at the right version (host-arch only — cross-arch
    # binaries can't be exec'd to query version).
    if [ "$triple" = "$(detect_host_triple)" ]; then
      local current
      current=$("$dest" --version 2>/dev/null | awk '{print $2}' || true)
      if [ "v$current" = "$RTK_VERSION" ]; then
        echo "[fetch-rtk] $triple already at $RTK_VERSION, skipping"
        return 0
      fi
    else
      # Cross-arch — trust file's existence (re-fetch only on --force).
      echo "[fetch-rtk] $triple already present, skipping (use --force to redo)"
      return 0
    fi
  fi

  local url="https://github.com/rtk-ai/rtk/releases/download/$RTK_VERSION/$archive"
  local tmp
  tmp=$(mktemp -d)
  echo "[fetch-rtk] downloading $url"
  curl -fsSL -o "$tmp/$archive" "$url"
  tar -xzf "$tmp/$archive" -C "$tmp"
  if [ ! -f "$tmp/rtk" ]; then
    echo "[fetch-rtk] expected $tmp/rtk after extract, got:" >&2
    ls "$tmp" >&2
    rm -rf "$tmp"
    return 1
  fi
  mv "$tmp/rtk" "$dest"
  chmod 0755 "$dest"
  rm -rf "$tmp"

  if [[ "$triple" == *-apple-darwin ]]; then
    # Ad-hoc sign so Gatekeeper accepts the bundle. `-s -` is the
    # "ad-hoc" identity; no cert needed. Without this, macOS kills
    # the spawn with `Killed: 9` on first launch.
    if command -v codesign >/dev/null 2>&1; then
      codesign -s - --force --preserve-metadata=identifier,entitlements,flags "$dest" || true
    fi
  fi
  echo "[fetch-rtk] wrote $dest"
}

for t in "${TARGETS[@]}"; do
  fetch_one "$t"
done
