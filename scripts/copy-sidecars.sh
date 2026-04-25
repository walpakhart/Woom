#!/usr/bin/env bash
# Copy freshly-built sidecar binaries from cargo's target dir into
# `apps/desktop/src-tauri/binaries/<name>-<triple>` so Tauri's bundler
# can pick them up via `externalBin` in `tauri.conf.json`.
#
# Usage: scripts/copy-sidecars.sh debug | release
#
# Triple is inferred from the host arch (`uname -m`). Universal builds
# get the per-arch artifacts merged via `lipo` upstream of this script
# (handled by `tauri build --target universal-apple-darwin`); for the
# happy path of a single-arch local build, this is enough.

set -euo pipefail

PROFILE="${1:-release}"
case "$PROFILE" in
  debug)   TARGET_DIR="apps/desktop/src-tauri/target/debug" ;;
  release) TARGET_DIR="apps/desktop/src-tauri/target/release" ;;
  *) echo "usage: $0 debug|release" >&2; exit 1 ;;
esac

BINARIES_DIR="apps/desktop/src-tauri/binaries"
mkdir -p "$BINARIES_DIR"

ARCH="$(uname -m)"
case "$ARCH" in
  arm64)  TRIPLE="aarch64-apple-darwin" ;;
  x86_64) TRIPLE="x86_64-apple-darwin" ;;
  *) echo "unsupported arch: $ARCH" >&2; exit 1 ;;
esac

SIDECARS=(forgehold-jira forgehold-github forgehold-memory forgehold-sentry forgehold-app)

for name in "${SIDECARS[@]}"; do
  src="$TARGET_DIR/$name"
  dst="$BINARIES_DIR/$name-$TRIPLE"
  if [[ ! -f "$src" ]]; then
    echo "  skip: $src not built (cargo failed?)" >&2
    continue
  fi
  cp "$src" "$dst"
  chmod +x "$dst"
  echo "  ✓ $name → $dst"
done
