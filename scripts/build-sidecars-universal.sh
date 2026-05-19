#!/usr/bin/env bash
# Build sidecar binaries (woom-jira / woom-github / woom-memory / woom-sentry /
# woom-app) for BOTH macOS architectures so `tauri build --target
# universal-apple-darwin` can lipo them into the final universal `.app`.
#
# Usage: scripts/build-sidecars-universal.sh
#
# Produces:
#   apps/desktop/src-tauri/binaries/<name>-aarch64-apple-darwin
#   apps/desktop/src-tauri/binaries/<name>-x86_64-apple-darwin
#
# Requires rustup with both `aarch64-apple-darwin` and `x86_64-apple-darwin`
# targets installed (added on demand below).

set -euo pipefail

MANIFEST="apps/desktop/src-tauri/Cargo.toml"
BINARIES_DIR="apps/desktop/src-tauri/binaries"
SIDECARS=(woom-jira woom-github woom-memory woom-sentry woom-app)
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)

mkdir -p "$BINARIES_DIR"

for triple in "${TARGETS[@]}"; do
  echo "==> rustup target add $triple"
  rustup target add "$triple" >/dev/null

  echo "==> cargo build --release --target $triple"
  cargo build \
    --manifest-path "$MANIFEST" \
    --release \
    --target "$triple" \
    -p woom-jira -p woom-github -p woom-memory -p woom-sentry -p woom-app

  for name in "${SIDECARS[@]}"; do
    src="apps/desktop/src-tauri/target/$triple/release/$name"
    dst="$BINARIES_DIR/$name-$triple"
    if [[ ! -f "$src" ]]; then
      echo "  ✗ missing: $src" >&2
      exit 1
    fi
    cp "$src" "$dst"
    chmod +x "$dst"
    echo "  ✓ $name → $dst"
  done
done
